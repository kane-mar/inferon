//! Lying-guarded-upstream mock (ITEM-027 AC3 smoke scenario). Runs inside the
//! same binary (`inferon --mock-upstream`) so the deployment unit stays one
//! image; the smoke stack starts it under the `mock` compose profile.
//!
//! Behaviour matrix (what the smoke asserts):
//!   - cheap-tier guarded request (gpt-4o-mini + json_schema + stream) →
//!     a *lying* complete SSE whose content violates the answer schema;
//!   - the guard's frontier replay (deepseek-v4-pro + json_schema + stream) →
//!     a schema-valid SSE, so the fallback can succeed;
//!   - everything else (the smoke's ordinary chat checks) → 502, mimicking
//!     the unreachable upstream the smoke historically relied on.

use axum::{routing::post, Json, Router};
use serde_json::Value;
use std::net::SocketAddr;

fn sse_with_content(content_json: &str) -> String {
    let frame = serde_json::json!({
        "id": "1",
        "choices": [{"delta": {"content": content_json}}]
    });
    format!("data: {frame}\n\ndata: [DONE]\n\n")
}

async fn chat_completions(Json(payload): Json<Value>) -> axum::response::Response {
    let has_contract = crate::guard::request_schema(&payload).is_some();
    let streaming = payload["stream"].as_bool().unwrap_or(false);
    let model = payload["model"].as_str().unwrap_or("");
    let body = |content: &str| {
        axum::response::Response::builder()
            .header("content-type", "text/event-stream")
            .body(axum::body::Body::from(sse_with_content(content).to_string()))
            .unwrap()
    };
    if has_contract && streaming && crate::guard::is_cheap_tier(model) {
        body(r#"{"other":1}"#) // lying cheap answer: violates the answer schema
    } else if has_contract && streaming && !crate::guard::is_cheap_tier(model) {
        body(r#"{"answer":"42"}"#) // frontier replay honours the contract
    } else {
        axum::response::Response::builder()
            .status(502)
            .body(axum::body::Body::from("mock upstream boom"))
            .unwrap()
    }
}

pub async fn serve(port: u16) -> std::io::Result<()> {
    let app = Router::new().route("/v1/chat/completions", post(chat_completions));
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("mock upstream listening on {addr}");
    axum::serve(listener, app).await
}
