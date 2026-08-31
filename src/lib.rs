use axum::{
    body::Body,
    extract::{Request, State},
    http::{header::CONTENT_TYPE, HeaderValue, StatusCode},
    response::Response,
    routing::{get, post},
    Router,
};
use futures_util::{Stream, StreamExt};
use reqwest::Client;
use serde_json::Value;
use std::{sync::Arc, time::Instant};

const MAX_REQUEST_BYTES: usize = 10 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub http_client: Client,
    pub upstream_url: String,
    pub upstream_api_key: String,
}

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/chat/completions", post(proxy_chat_completions))
        .with_state(state)
}

async fn healthz() -> &'static str {
    "ok"
}

/// Proxy a chat completion request to the configured upstream.
///
/// Streaming requests pipe upstream bytes through unchanged (zero-copy) as
/// text/event-stream; non-streaming requests pass the JSON body through with
/// usage intact. Transport errors become 502; upstream HTTP error statuses
/// are forwarded as-is.
async fn proxy_chat_completions(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Result<Response, StatusCode> {
    let body_bytes = axum::body::to_bytes(req.into_body(), MAX_REQUEST_BYTES)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    let payload: Value =
        serde_json::from_slice(&body_bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    let is_stream = payload["stream"].as_bool().unwrap_or(false);

    let started = Instant::now();
    let upstream_request = state
        .http_client
        .post(&state.upstream_url)
        .header("Authorization", format!("Bearer {}", state.upstream_api_key))
        .header("Content-Type", "application/json")
        .body(body_bytes);

    let upstream = match upstream_request.send().await {
        Ok(response) => response,
        Err(e) => {
            tracing::error!("upstream request failed: {e}");
            return Ok(upstream_error_response());
        }
    };

    let status = upstream.status();
    if !status.is_success() {
        return forward_raw(upstream).await;
    }
    if is_stream {
        Ok(stream_response(upstream, started))
    } else {
        forward_json(upstream).await
    }
}

/// Forward an upstream error response unchanged (status + body + content-type).
async fn forward_raw(upstream: reqwest::Response) -> Result<Response, StatusCode> {
    let status = upstream.status();
    let content_type = upstream.headers().get(CONTENT_TYPE).cloned();
    let bytes = upstream.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = status;
    if let Some(content_type) = content_type {
        response.headers_mut().insert(CONTENT_TYPE, content_type);
    }
    Ok(response)
}

/// Forward a successful non-streaming response as JSON.
async fn forward_json(upstream: reqwest::Response) -> Result<Response, StatusCode> {
    let status = upstream.status();
    let bytes = upstream.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    Ok(response)
}

/// OpenAI-style error body for upstream transport failures (AC5: 502 with body).
fn upstream_error_response() -> Response {
    let body = r#"{"error":{"message":"upstream request failed","type":"upstream_error"}}"#;
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = StatusCode::BAD_GATEWAY;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

/// Pipe an upstream byte stream through as text/event-stream, measuring and
/// logging time-to-first-token (first upstream chunk) on the way.
fn stream_response(upstream: reqwest::Response, started: Instant) -> Response {
    let status = upstream.status();
    let byte_stream = tap_ttft(upstream.bytes_stream(), started);
    let mut response = Response::new(Body::from_stream(byte_stream));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    response
}

/// Tag each upstream chunk with its arrival time; record TTFT on the first one.
/// Chunks are yielded immediately (no buffering), so added latency is ~0.
fn tap_ttft(
    byte_stream: impl Stream<Item = Result<axum::body::Bytes, reqwest::Error>>,
    started: Instant,
) -> impl Stream<Item = Result<axum::body::Bytes, std::io::Error>> {
    async_stream::stream! {
        let mut stream = Box::pin(byte_stream);
        let mut first = true;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    if first {
                        let ttft_ms = started.elapsed().as_millis() as u64;
                        tracing::debug!(ttft_ms, "first upstream chunk");
                        first = false;
                    }
                    yield Ok(bytes);
                }
                Err(e) => {
                    tracing::error!("upstream stream error: {e}");
                    break;
                }
            }
        }
    }
}

pub fn parse_port(value: Option<&str>) -> u16 {
    value
        .and_then(|v| v.parse().ok())
        .unwrap_or(8080)
}

pub async fn ping_health(port: u16) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let Ok(mut stream) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await else {
        return false;
    };
    if stream
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .is_err()
    {
        return false;
    }
    let mut response = String::new();
    stream.read_to_string(&mut response).await.is_ok() && response.starts_with("HTTP/1.1 200")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    const SSE_BODY: &str = "data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: [DONE]\n\n";

    async fn mock_upstream() -> String {
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(|payload: axum::Json<serde_json::Value>| async move {
                match payload["model"].as_str() {
                    Some("fail") => axum::response::Response::builder()
                        .status(500)
                        .body(Body::from("boom"))
                        .unwrap(),
                    Some("rate") => axum::response::Response::builder()
                        .status(429)
                        .body(Body::from("rate limited"))
                        .unwrap(),
                    _ if payload["stream"].as_bool().unwrap_or(false) => {
                        axum::response::Response::builder()
                            .header("content-type", "text/event-stream")
                            .body(Body::from(SSE_BODY))
                            .unwrap()
                    }
                    _ => axum::response::Response::builder()
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"id":"1","choices":[{"message":{"content":"hi"}}],"usage":{"prompt_tokens":5,"completion_tokens":2}}"#,
                        ))
                        .unwrap(),
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}/v1/chat/completions")
    }

    fn test_state(upstream_url: &str) -> std::sync::Arc<AppState> {
        std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: upstream_url.to_string(),
            upstream_api_key: "test-key".to_string(),
        })
    }

    async fn chat_request(body: &str) -> axum::response::Response {
        build_router(test_state(&mock_upstream().await))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn healthz_returns_200_ok() {
        let response = build_router(test_state("http://127.0.0.1:1/"))
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[test]
    fn parse_port_defaults_to_8080() {
        assert_eq!(parse_port(None), 8080);
    }

    #[test]
    fn parse_port_reads_explicit_value() {
        assert_eq!(parse_port(Some("9090")), 9090);
    }

    #[tokio::test]
    async fn ping_health_returns_true_when_server_is_up() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = build_router(test_state("http://127.0.0.1:1/"));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert!(ping_health(port).await);
    }

    #[tokio::test]
    async fn ping_health_returns_false_when_nothing_is_listening() {
        assert!(!ping_health(1).await);
    }

    #[tokio::test]
    async fn stream_request_passes_sse_through_byte_identical() {
        let response = chat_request(
            r#"{"model":"deepseek-chat","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/event-stream"
        );
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), SSE_BODY.as_bytes());
    }

    #[tokio::test]
    async fn non_stream_request_passes_json_through_with_usage() {
        let response = chat_request(
            r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/json"
        );
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["usage"]["prompt_tokens"], 5);
        assert_eq!(json["usage"]["completion_tokens"], 2);
    }

    #[tokio::test]
    async fn upstream_transport_failure_returns_502() {
        let state = test_state("http://127.0.0.1:1/v1/chat/completions");
        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::BAD_GATEWAY);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["type"], "upstream_error");
    }

    #[tokio::test]
    async fn upstream_http_error_is_forwarded_unchanged() {
        let response = chat_request(
            r#"{"model":"rate","messages":[{"role":"user","content":"hi"}]}"#,
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
    }
}
