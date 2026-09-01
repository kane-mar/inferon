use axum::{
    body::Body,
    extract::{Request, State},
    http::{header::{AUTHORIZATION, CONTENT_TYPE}, HeaderValue, StatusCode},
    response::Response,
    routing::{get, post},
    Router,
};
use futures_util::{Stream, StreamExt};
use reqwest::Client;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::Arc,
    time::Instant,
};
use uuid::Uuid;

const MAX_REQUEST_BYTES: usize = 10 * 1024 * 1024;

/// A resolved tenant. The upstream key is Inferon-held (the tenant's master
/// key); client-presented API keys are only ever stored/compared as SHA-256.
#[derive(Clone, Debug)]
pub struct Tenant {
    pub id: Uuid,
    pub upstream_openai_key: Option<String>,
}

/// Lookup seam: resolve a tenant from the SHA-256 hash of a client API key.
/// InMemoryTenantStore backs unit tests; PostgresTenantStore is the production
/// store (schema 001: tenants.api_key_hash UNIQUE, upstream_openai_key TEXT).
#[async_trait::async_trait]
pub trait TenantStore: Send + Sync {
    async fn tenant_by_api_key_hash(
        &self,
        api_key_hash: &str,
    ) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>>;
}

#[derive(Default)]
pub struct InMemoryTenantStore {
    tenants: HashMap<String, Tenant>,
}

impl InMemoryTenantStore {
    pub fn new(tenants: HashMap<String, Tenant>) -> Self {
        Self { tenants }
    }
}

#[async_trait::async_trait]
impl TenantStore for InMemoryTenantStore {
    async fn tenant_by_api_key_hash(
        &self,
        api_key_hash: &str,
    ) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.tenants.get(api_key_hash).cloned())
    }
}

pub struct PostgresTenantStore {
    pool: sqlx::PgPool,
}

impl PostgresTenantStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl TenantStore for PostgresTenantStore {
    async fn tenant_by_api_key_hash(
        &self,
        api_key_hash: &str,
    ) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>> {
        let row = sqlx::query_as::<_, (Uuid, Option<String>)>(
            "SELECT id, upstream_openai_key FROM tenants WHERE api_key_hash = $1",
        )
        .bind(api_key_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id, upstream_openai_key)| Tenant {
            id,
            upstream_openai_key,
        }))
    }
}

/// Response extension carrying the resolved tenant id — the hook ITEM-005
/// telemetry reads when writing traces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TenantId(pub Uuid);

#[derive(Clone)]
pub struct AppState {
    pub http_client: Client,
    pub upstream_url: String,
    pub tenant_store: Arc<dyn TenantStore>,
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
/// Authenticates the caller via `Authorization: Bearer <key>` (SHA-256 lookup
/// against the tenant store), swaps the upstream auth header to the tenant's
/// master key, then forwards: streaming requests pipe upstream bytes through
/// unchanged (zero-copy) as text/event-stream; non-streaming requests pass the
/// JSON body through with usage intact. Transport errors become 502; upstream
/// HTTP error statuses are forwarded as-is. Every response after auth carries
/// the tenant id in its extensions for telemetry (ITEM-005).
async fn proxy_chat_completions(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Response {
    let tenant = match authenticate(&state, req.headers()).await {
        Ok(Some(tenant)) => tenant,
        Ok(None) => {
            return json_error(
                StatusCode::UNAUTHORIZED,
                "invalid_api_key",
                "missing or invalid Authorization header",
            )
        }
        Err(e) => {
            tracing::error!("tenant lookup failed: {e}");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "tenant_store_error",
                "tenant lookup failed",
            );
        }
    };
    let tenant_id = tenant.id;

    let body_bytes = match axum::body::to_bytes(req.into_body(), MAX_REQUEST_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return attach_tenant(
                json_error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "payload_too_large",
                    "request body exceeds 10MB",
                ),
                tenant_id,
            )
        }
    };
    let payload: Value = match serde_json::from_slice(&body_bytes) {
        Ok(payload) => payload,
        Err(_) => {
            return attach_tenant(
                json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_json",
                    "request body must be valid JSON",
                ),
                tenant_id,
            )
        }
    };
    let is_stream = payload["stream"].as_bool().unwrap_or(false);
    let Some(upstream_key) = tenant.upstream_openai_key.as_deref() else {
        return attach_tenant(
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "tenant_misconfigured",
                "tenant has no upstream key configured",
            ),
            tenant_id,
        );
    };

    let started = Instant::now();
    let upstream_request = state
        .http_client
        .post(&state.upstream_url)
        .header("Authorization", format!("Bearer {upstream_key}"))
        .header("Content-Type", "application/json")
        .body(body_bytes);

    let upstream = match upstream_request.send().await {
        Ok(response) => response,
        Err(e) => {
            tracing::error!("upstream request failed: {e}");
            return attach_tenant(upstream_error_response(), tenant_id);
        }
    };

    let status = upstream.status();
    if !status.is_success() {
        return attach_tenant(forward_raw(upstream).await, tenant_id);
    }
    if is_stream {
        attach_tenant(stream_response(upstream, started), tenant_id)
    } else {
        attach_tenant(forward_json(upstream).await, tenant_id)
    }
}

/// Resolve the caller's tenant from the Authorization header.
/// Ok(Some(tenant)) = authenticated; Ok(None) = missing/invalid credentials;
/// Err = store failure (caller should see 500, never a misleading 401).
async fn authenticate(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>> {
    let Some(auth) = headers.get(AUTHORIZATION) else {
        return Ok(None);
    };
    let Ok(auth) = auth.to_str() else {
        return Ok(None);
    };
    let Some(key) = auth.strip_prefix("Bearer ") else {
        return Ok(None);
    };
    if key.is_empty() {
        return Ok(None);
    }
    state.tenant_store.tenant_by_api_key_hash(&tenant_hash(key)).await
}

/// SHA-256 hex digest of an API key — the only form of a client key that is
/// ever stored, compared, or logged (AC5).
fn tenant_hash(key: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(key.as_bytes()))
}

/// OpenAI-style JSON error response.
fn json_error(status: StatusCode, err_type: &str, message: &str) -> Response {
    let body = serde_json::json!({"error": {"message": message, "type": err_type}}).to_string();
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

/// Stamp a response with the tenant id for the telemetry layer (ITEM-005).
fn attach_tenant(mut response: Response, tenant_id: Uuid) -> Response {
    response.extensions_mut().insert(TenantId(tenant_id));
    response
}

/// Forward an upstream error response unchanged (status + body + content-type).
async fn forward_raw(upstream: reqwest::Response) -> Response {
    let status = upstream.status();
    let content_type = upstream.headers().get(CONTENT_TYPE).cloned();
    match upstream.bytes().await {
        Ok(bytes) => {
            let mut response = Response::new(Body::from(bytes));
            *response.status_mut() = status;
            if let Some(content_type) = content_type {
                response.headers_mut().insert(CONTENT_TYPE, content_type);
            }
            response
        }
        Err(e) => {
            tracing::error!("upstream body read failed: {e}");
            upstream_error_response()
        }
    }
}

/// Forward a successful non-streaming response as JSON.
async fn forward_json(upstream: reqwest::Response) -> Response {
    let status = upstream.status();
    match upstream.bytes().await {
        Ok(bytes) => {
            let mut response = Response::new(Body::from(bytes));
            *response.status_mut() = status;
            response
                .headers_mut()
                .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            response
        }
        Err(e) => {
            tracing::error!("upstream body read failed: {e}");
            upstream_error_response()
        }
    }
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
    const CLIENT_KEY: &str = "sk-client-dev-key";
    const MASTER_KEY: &str = "sk-tenant-master-key";
    const TENANT_ID: &str = "11111111-1111-1111-1111-111111111111";

    fn tenant_hash(key: &str) -> String {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(key.as_bytes()))
    }

    fn seeded_store() -> InMemoryTenantStore {
        let mut tenants = std::collections::HashMap::new();
        tenants.insert(
            tenant_hash(CLIENT_KEY),
            Tenant {
                id: uuid::Uuid::parse_str(TENANT_ID).unwrap(),
                upstream_openai_key: Some(MASTER_KEY.to_string()),
            },
        );
        InMemoryTenantStore { tenants }
    }

    struct FailingStore;

    #[async_trait::async_trait]
    impl TenantStore for FailingStore {
        async fn tenant_by_api_key_hash(
            &self,
            _api_key_hash: &str,
        ) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>> {
            Err("tenant store unavailable".into())
        }
    }

    async fn mock_upstream() -> String {
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(
                |headers: axum::http::HeaderMap, payload: axum::Json<serde_json::Value>| async move {
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
                        _ => {
                            // Echo the Authorization header we received so tests
                            // can prove the proxy swapped the client key for the
                            // tenant's master upstream key.
                            let auth = headers
                                .get("authorization")
                                .and_then(|v| v.to_str().ok())
                                .unwrap_or("");
                            let body = format!(
                                r#"{{"echoed_auth":"{auth}","usage":{{"prompt_tokens":5,"completion_tokens":2}}}}"#
                            );
                            axum::response::Response::builder()
                                .header("content-type", "application/json")
                                .body(Body::from(body))
                                .unwrap()
                        }
                    }
                },
            ),
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
            tenant_store: std::sync::Arc::new(seeded_store()),
        })
    }

    fn failing_state(upstream_url: &str) -> std::sync::Arc<AppState> {
        std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: upstream_url.to_string(),
            tenant_store: std::sync::Arc::new(FailingStore),
        })
    }

    async fn chat_request(body: &str, auth: Option<&str>) -> axum::response::Response {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("content-type", "application/json");
        if let Some(key) = auth {
            builder = builder.header("authorization", format!("Bearer {key}"));
        }
        build_router(test_state(&mock_upstream().await))
            .oneshot(builder.body(Body::from(body.to_string())).unwrap())
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
            Some(CLIENT_KEY),
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
            Some(CLIENT_KEY),
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
                    .header("authorization", format!("Bearer {CLIENT_KEY}"))
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
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn missing_auth_header_returns_401() {
        let response = chat_request(
            r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
            None,
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
        assert!(response.extensions().get::<TenantId>().is_none());
    }

    #[tokio::test]
    async fn unknown_api_key_returns_401() {
        let response = chat_request(
            r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
            Some("sk-nope"),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn valid_key_swaps_upstream_auth_header() {
        let response = chat_request(
            r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["echoed_auth"], format!("Bearer {MASTER_KEY}"));
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            !body_str.contains(CLIENT_KEY),
            "client key must never reach the upstream"
        );
    }

    #[tokio::test]
    async fn tenant_id_is_attached_to_success_response() {
        let response = chat_request(
            r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let tenant_id = response.extensions().get::<TenantId>().copied();
        assert_eq!(
            tenant_id,
            Some(TenantId(uuid::Uuid::parse_str(TENANT_ID).unwrap()))
        );
    }

    #[tokio::test]
    async fn non_json_body_returns_400_with_tenant_attached() {
        let response = chat_request("this is not json", Some(CLIENT_KEY)).await;

        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
        let tenant_id = response.extensions().get::<TenantId>().copied();
        assert_eq!(
            tenant_id,
            Some(TenantId(uuid::Uuid::parse_str(TENANT_ID).unwrap()))
        );
    }

    #[tokio::test]
    async fn tenant_store_failure_returns_500() {
        let state = failing_state("http://127.0.0.1:1/v1/chat/completions");
        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {CLIENT_KEY}"))
                    .body(Body::from(
                        r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
