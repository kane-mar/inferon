//! Inferon — an OpenAI/Anthropic-compatible LLM proxy with cost telemetry, a
//! budget breaker, a schema guard, and a tenant/operator control plane.
//!
//! The crate is layered; each module has one job:
//!
//! - **Composition root** (this file): `AppState`, `build_router`, the shared
//!   seams (`authenticate`, `json_error`, `sha256_hex`).
//! - **Data plane** (`proxy`): the request path — resolve the provider, apply
//!   the router/budget/guard/affinity/policy controls, forward upstream, trace.
//! - **Control plane** (`dashboard`, `dashboard::operator`): the tenant and
//!   operator HTTP surfaces.
//! - **Ingress seams** (`auth`, `edge`): OAuth/sessions and edge-node ingest.
//! - **Routing intelligence** (`router`, `budget`, `routing`, `guard`,
//!   `affinity`, `policy`, `quality`, `linter`, `cost`, `usage`).
//! - **Stores** (`tenant`, `providers`, `models`, `routing`, `budget`,
//!   `affinity`, `policy`, `telemetry`), each a trait seam with a Postgres and
//!   an in-memory implementation.
//! - **Shared** (`errors`, `metrics`, `clock`, `sse`, `session`, `secret`).
//! - **Mocks** (`mock_github`, `mock_upstream`) power the smoke stack.

// Every request-path helper returns its early-return HTTP response directly as
// the error (`Result<T, Response>`) — the axum idiom. Boxing the response at
// ~10 signatures would only add noise.
#![allow(clippy::result_large_err)]

// Control-plane surfaces.
pub mod auth;
pub mod dashboard;
pub mod edge;
// Data plane.
pub mod proxy;
// Routing intelligence.
pub mod affinity;
pub mod alerts;
pub mod budget;
pub mod cost;
pub mod guard;
pub mod linter;
pub mod policy;
pub mod quality;
pub mod router;
pub mod routing;
pub mod usage;
// Stores (trait seam + Postgres + in-memory).
pub mod models;
pub mod providers;
pub mod tenant;
pub mod telemetry;
// Shared primitives.
pub mod clock;
pub mod embed;
pub mod errors;
pub mod metrics;
pub mod secret;
pub mod session;
pub mod sse;
// Smoke-stack mocks (same binary).
pub mod mock_github;
pub mod mock_upstream;

pub use tenant::{ApiKeyInfo, InMemoryTenantStore, PostgresTenantStore, Tenant, TenantStore};
pub use telemetry::{load_system_prompt_counts, telemetry_worker, PostgresTraceSink, TraceMessage, TraceSink};
pub use usage::{usage_from_sse_bytes, Usage};

use crate::proxy::{healthz, list_models, proxy_chat_completions, proxy_messages, quality_signal};

use axum::{
    body::Body,
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        HeaderValue, StatusCode,
    },
    response::Response,
    routing::{get, post},
    Router,
};
use reqwest::Client;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

pub(crate) const MAX_REQUEST_BYTES: usize = 10 * 1024 * 1024;

pub struct AppState {
    pub http_client: Client,
    pub upstream_url: String,
    pub tenant_store: Arc<dyn TenantStore>,
    /// Tenant-provisioned providers (ITEM-043): resolved per request, falling
    /// back to the tenant's shared upstream key when none is configured.
    pub provider_store: Arc<dyn providers::ProviderStore>,
    /// Per-tenant model selection (ITEM-044): a disabled model is rejected with
    /// a typed error; an unconfigured model is unrestricted.
    pub model_store: Arc<dyn models::ModelStore>,
    /// Per-tenant fallback chains (ITEM-045): tried on retryable upstream
    /// failures. Empty = no chain (behaviour unchanged).
    pub routing_store: Arc<dyn routing::RoutingStore>,
    pub budget_store: Arc<dyn budget::BudgetStore>,
    /// ITEM-063: per-prefix warm-target lookup for cache-affinity routing.
    pub affinity_store: Arc<dyn affinity::AffinityStore>,
    /// ITEM-064: the tenant's active adaptive policy (versioned).
    pub policy_store: Arc<dyn policy::PolicyStore>,
    /// ITEM-065: enrolled edge nodes (control plane).
    pub edge_store: Arc<dyn edge::EdgeNodeStore>,
    pub telemetry_tx: mpsc::Sender<TraceMessage>,
    /// Operator credential (env `OPERATOR_API_KEY`): grants the platform-wide
    /// dashboard views. `None` disables operator access entirely (ITEM-013).
    pub operator_key: Option<String>,
    /// Human accounts (GitHub OAuth + signed sessions, ITEM-035). `None` when
    /// OAuth is unconfigured — `/auth/login` answers 503 then (AC3).
    pub accounts: Option<Arc<auth::Accounts>>,
    pub db: sqlx::PgPool,
}

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/chat/completions", post(proxy_chat_completions))
        .route("/v1/messages", post(proxy_messages))
        .route("/v1/edge/traces", post(edge::ingest))
        .route("/v1/edge/config", get(edge::config))
        .route("/v1/quality", post(quality_signal))
        .route("/v1/models", get(list_models))
        .route("/v1/dashboard/api/summary", get(dashboard::summary))
        .route("/v1/dashboard/api/providers", get(dashboard::tenant_providers).post(dashboard::tenant_provider_create))
        .route(
            "/v1/dashboard/api/providers/:provider_id/remove",
            post(dashboard::tenant_provider_remove),
        )
        .route(
            "/v1/dashboard/api/providers/:provider_id",
            post(dashboard::tenant_provider_update),
        )
        .route(
            "/v1/dashboard/api/providers/test",
            post(dashboard::tenant_provider_probe),
        )
        .route(
            "/v1/dashboard/api/providers/:provider_id/test",
            post(dashboard::tenant_provider_test),
        )
        .route(
            "/v1/dashboard/api/providers/:provider_id/default",
            post(dashboard::tenant_provider_default),
        )
        .route("/v1/dashboard/api/models", get(dashboard::tenant_models))
        .route(
            "/v1/dashboard/api/providers/:provider_id/models/:model_id",
            post(dashboard::tenant_provider_model_update),
        )
        .route(
            "/v1/dashboard/api/providers/:provider_id/models",
            get(dashboard::tenant_provider_models),
        )
        .route("/v1/dashboard/api/routing", get(dashboard::tenant_routing).post(dashboard::tenant_routing_set))
        .route("/v1/dashboard/api/config", get(dashboard::tenant_config).post(dashboard::tenant_config_set))
        .route("/v1/dashboard/api/by-model", get(dashboard::by_model))
        .route("/v1/dashboard/api/by-prompt", get(dashboard::by_prompt))
        .route("/v1/dashboard/api/by-provider", get(dashboard::by_provider))
        .route("/v1/dashboard/api/by-key", get(dashboard::by_key))
        .route(
            "/v1/dashboard/api/edge/nodes",
            get(dashboard::tenant_edge_nodes).post(dashboard::tenant_edge_node_create),
        )
        .route(
            "/v1/dashboard/api/edge/nodes/:node_id/revoke",
            post(dashboard::tenant_edge_node_revoke),
        )
        .route("/v1/dashboard/api/quality", get(dashboard::quality))
        .route("/v1/dashboard/api/waste", get(dashboard::waste))
        .route("/v1/dashboard/api/timeseries", get(dashboard::timeseries))
        .route("/v1/dashboard/api/guard", get(dashboard::guard))
        .route("/v1/dashboard/api/operator/tenants", get(dashboard::operator_tenants))
        .route("/v1/dashboard/api/operator/summary", get(dashboard::operator_summary))
        .route("/v1/dashboard/api/operator/by-model", get(dashboard::operator_by_model))
        .route("/v1/dashboard/api/operator/by-prompt", get(dashboard::operator_by_prompt))
        .route("/v1/dashboard/api/operator/alerts", get(dashboard::operator_alerts))
        .route("/v1/dashboard/api/operator/guard", get(dashboard::operator_guard))
        .route("/v1/dashboard/api/operator/metrics", get(dashboard::operator_metrics))
        .route(
            "/v1/dashboard/api/operator/activation",
            get(dashboard::operator_activation),
        )
        .route(
            "/v1/dashboard/api/operator/policy/:tenant_id",
            get(dashboard::operator_policy),
        )
        .route(
            "/v1/dashboard/api/operator/policy/:tenant_id/freeze",
            post(dashboard::operator_policy_freeze),
        )
        .route(
            "/v1/dashboard/api/operator/policy/:tenant_id/rollback/:version",
            post(dashboard::operator_policy_rollback),
        )
        .route(
            "/v1/dashboard/api/operator/keys",
            get(dashboard::operator_keys).post(dashboard::operator_keys_create),
        )
        .route(
            "/v1/dashboard/api/operator/keys/:key_id/revoke",
            post(dashboard::operator_key_revoke),
        )
        .route(
            "/v1/dashboard/api/keys",
            get(dashboard::tenant_keys).post(dashboard::tenant_key_create),
        )
        .route(
            "/v1/dashboard/api/keys/:key_id/revoke",
            post(dashboard::tenant_key_revoke),
        )
        .route(
            "/v1/dashboard/api/keys/:key_id/limit",
            post(dashboard::tenant_key_limit),
        )
        .route(
            "/v1/dashboard/api/members",
            get(dashboard::tenant_members).post(dashboard::tenant_member_invite),
        )
        .route(
            "/v1/dashboard/api/members/:user_id/remove",
            post(dashboard::tenant_member_remove),
        )
        .route("/auth/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .with_state(state)
}

























/// Resolve the caller's tenant from the Authorization header.
/// Ok(Some(tenant)) = authenticated; Ok(None) = missing/invalid credentials;
/// Err = store failure (caller should see 500, never a misleading 401).
pub(crate) async fn authenticate(
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
    state.tenant_store.tenant_by_api_key_hash(&sha256_hex(key)).await
}

/// SHA-256 hex digest. API keys are only ever stored/compared/logged in this
/// form (AC5); the system-prompt hash (linter rule B) reuses the same digest.
pub(crate) fn sha256_hex(input: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(input.as_bytes()))
}

/// Key prefix for client API keys (ITEM-034 mints). Raw keys are shown once
/// at mint time; only the SHA-256 hash is persisted.
pub(crate) const CLIENT_KEY_PREFIX: &str = "sk-inferon-";

/// Mint a fresh raw client key (128 bits of randomness, UUID v4 backend).
pub(crate) fn mint_client_key() -> String {
    format!("{CLIENT_KEY_PREFIX}{}", Uuid::new_v4().simple())
}


/// OpenAI-style JSON error response.
pub(crate) fn json_error(status: StatusCode, err_type: &str, message: &str) -> Response {
    let body = serde_json::json!({"error": {"message": message, "type": err_type}}).to_string();
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
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
    use crate::proxy::{resolve_route, Surface};
    use crate::usage::system_prompt_hash;
    use serde_json::{json, Value};
    use axum::body::Body;
    use axum::http::Request;
    use std::collections::HashMap;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tower::ServiceExt;

    const SSE_BODY: &str = "data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: {\"id\":\"1\",\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\ndata: [DONE]\n\n";
    /// SSE body whose single delta content is `content_json` (a JSON object
    /// double-encoded inside the delta string, like a real chat completion).
    fn delta_only_stream(content_json: &str) -> String {
        let frame = serde_json::json!({
            "id": "1",
            "choices": [{"delta": {"content": content_json}}]
        });
        format!("data: {frame}\n\ndata: [DONE]\n\n")
    }

    const SSE_BODY_NO_USAGE: &str =
        "data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: [DONE]\n\n";
    const CLIENT_KEY: &str = "sk-client-dev-key";
    const MASTER_KEY: &str = "sk-tenant-master-key";
    const TENANT_ID: &str = "11111111-1111-1111-1111-111111111111";

    fn seeded_store() -> InMemoryTenantStore {
        let mut by_hash = std::collections::HashMap::new();
        by_hash.insert(
            sha256_hex(CLIENT_KEY),
            Tenant {
                id: uuid::Uuid::parse_str(TENANT_ID).unwrap(),
                upstream_openai_key: Some(MASTER_KEY.to_string()),
                api_key_id: None,
            },
        );
        InMemoryTenantStore::new(by_hash)
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

        async fn tenant_by_id(
            &self,
            _tenant_id: Uuid,
        ) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>> {
            Err("tenant store unavailable".into())
        }

        async fn insert_api_key(
            &self,
            _tenant_id: Uuid,
            _label: &str,
            _key_hash: &str,
        ) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
            Err("tenant store unavailable".into())
        }

        async fn list_api_keys(
            &self,
            _tenant_id: Uuid,
        ) -> Result<Vec<ApiKeyInfo>, Box<dyn std::error::Error + Send + Sync>> {
            Err("tenant store unavailable".into())
        }

        async fn revoke_api_key(
            &self,
            _key_id: Uuid,
        ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
            Err("tenant store unavailable".into())
        }

        async fn insert_tenant(
            &self,
            _upstream_key: Option<&str>,
        ) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
            Err("tenant store unavailable".into())
        }

        async fn revoke_tenant_key(
            &self,
            _tenant_id: Uuid,
            _key_id: Uuid,
        ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
            Err("tenant store unavailable".into())
        }

        async fn set_key_spend_limit(
            &self,
            _tenant_id: Uuid,
            _key_id: Uuid,
            _limit_cents: i64,
        ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
            Err("tenant store unavailable".into())
        }
    }

    async fn mock_upstream() -> String {
        let app = axum::Router::new()
            .route(
                "/v1/models",
                axum::routing::get(|| async {
                    axum::Json(json!({"data":[{"id":"mock-model"}]}))
                }),
            )
            .route(
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
                        Some("nousage") => axum::response::Response::builder()
                            .header("content-type", "text/event-stream")
                            .body(Body::from(SSE_BODY_NO_USAGE))
                            .unwrap(),
                        Some("slow") => {
                            // Two SSE frames 300ms apart so a client can abort
                            // mid-stream while the tap is waiting for frame 2.
                            let body = async_stream::stream! {
                                yield Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
                                    b"data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
                                ));
                                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                                yield Ok(axum::body::Bytes::from_static(
                                    b"data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\ndata: [DONE]\n\n",
                                ));
                            };
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(axum::body::Body::from_stream(body))
                                .unwrap()
                        }
                        Some("miderror") => {
                            // One good frame, then — after headers have surely
                            // flushed — a mid-stream failure.
                            let body = async_stream::stream! {
                                yield Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
                                    b"data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
                                ));
                                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                                yield Err(std::io::Error::other("mock mid-stream failure"));
                            };
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(axum::body::Body::from_stream(body))
                                .unwrap()
                        }
                        Some("gpt-4o-mini") if {
                            // Cheap lie: frames arrive but [DONE] never does —
                            // the guard discards the buffer and falls back.
                            // Markers: "truncated" (container failure) and
                            // "boom" (fallback-failure test, ITEM-025 AC4).
                            matches!(
                                payload["messages"][0]["content"].as_str(),
                                Some("truncated" | "boom")
                            )
                        } => {
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from(
                                    "data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\ndata: {\"id\":\"1\",\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1}}\n\n",
                                ))
                                .unwrap()
                        }
                        Some("gpt-4o-mini") if {
                            // Content that satisfies the request's answer schema
                            // (well-formed JSON object) — must be flushed.
                            payload["messages"][0]["content"].as_str() == Some("valid-json")
                        } => {
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from(delta_only_stream(r#"{"answer":"42"}"#)))
                                .unwrap()
                        }
                        Some("gpt-4o-mini") if {
                            // Cheap lie: well-formed JSON that violates the
                            // answer schema ({"other":1}, missing required
                            // "answer") — must be discarded, never forwarded.
                            payload["messages"][0]["content"].as_str() == Some("wrong-shape")
                        } => {
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from(delta_only_stream(r#"{"other":1}"#)))
                                .unwrap()
                        }
                        Some("gpt-4o-mini") if {
                            // Cheap lie for the abort-during-fallback test:
                            // instant violation, so the replay starts at once.
                            payload["messages"][0]["content"].as_str() == Some("slow-fallback")
                        } => {
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from(delta_only_stream(r#"{"other":1}"#)))
                                .unwrap()
                        }
                        Some("gpt-4o-mini") if {
                            // Slow guarded stream: first frame, 300ms gap, then
                            // [DONE] — lets a test abort mid-buffer.
                            payload["messages"][0]["content"].as_str() == Some("guarded-slow")
                        } => {
                            let body = async_stream::stream! {
                                yield Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
                                    b"data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
                                ));
                                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                                yield Ok(axum::body::Bytes::from_static(
                                    b"data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\ndata: [DONE]\n\n",
                                ));
                            };
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(axum::body::Body::from_stream(body))
                                .unwrap()
                        }
                        Some("deepseek-v4-pro") if {
                            // Fallback failure (ITEM-025 AC4): the frontier
                            // replay itself answers 5xx — loud and clean.
                            payload["messages"][0]["content"].as_str() == Some("boom")
                        } => {
                            axum::response::Response::builder()
                                .status(500)
                                .body(Body::from("boom"))
                                .unwrap()
                        }
                        Some("deepseek-v4-pro") if {
                            // Default frontier replay (cheap original escalated
                            // to the deployment frontier): honours the contract.
                            let marker = payload["messages"][0]["content"].as_str();
                            matches!(marker, Some("wrong-shape" | "truncated"))
                        } => {
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from(delta_only_stream(r#"{"answer":"42"}"#)))
                                .unwrap()
                        }
                        Some("deepseek-v4-pro") if {
                            // Slow frontier replay: first frame, 300ms gap,
                            // then [DONE] — lets a test abort mid-replay.
                            payload["messages"][0]["content"].as_str() == Some("slow-fallback")
                        } => {
                            let body = async_stream::stream! {
                                yield Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
                                    b"data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
                                ));
                                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                                yield Ok(axum::body::Bytes::from_static(
                                    b"data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\ndata: [DONE]\n\n",
                                ));
                            };
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(axum::body::Body::from_stream(body))
                                .unwrap()
                        }
                        Some("gpt-4o") if {
                            // Frontier replay when the tenant's original request
                            // (pre-downgrade) was gpt-4o: honours the contract.
                            let marker = payload["messages"][0]["content"].as_str();
                            matches!(marker, Some("wrong-shape" | "truncated"))
                        } => {
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from(delta_only_stream(r#"{"answer":"42"}"#)))
                                .unwrap()
                        }
                        Some("content") => {
                            // A clean 2xx non-streaming completion with text.
                            axum::response::Response::builder()
                                .header("content-type", "application/json")
                                .body(Body::from(
                                    r#"{"choices":[{"message":{"content":"hello"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":1}}"#,
                                ))
                                .unwrap()
                        }
                        Some("empty") => {
                            // A 2xx with a blank completion — ITEM-055 `empty`.
                            axum::response::Response::builder()
                                .header("content-type", "application/json")
                                .body(Body::from(
                                    r#"{"choices":[{"message":{"content":""},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":0}}"#,
                                ))
                                .unwrap()
                        }
                        Some("refusal") => {
                            // A content-filter stop — ITEM-055 `refused`.
                            axum::response::Response::builder()
                                .header("content-type", "application/json")
                                .body(Body::from(
                                    r#"{"choices":[{"message":{"content":""},"finish_reason":"content_filter"}],"usage":{"prompt_tokens":3,"completion_tokens":0}}"#,
                                ))
                                .unwrap()
                        }
                        Some("nodone") => {
                            // Valid JSON frames but no [DONE] — used to prove
                            // unguarded traffic pipes a truncated body through
                            // unchanged (guard never engages without a schema).
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from(
                                    "data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\ndata: {\"id\":\"1\",\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1}}\n\n",
                                ))
                                .unwrap()
                        }
                        _ if payload["stream"].as_bool().unwrap_or(false) => {
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from(SSE_BODY))
                                .unwrap()
                        }
                        _ => {
                            // Echo the Authorization header and model we received
                            // so tests can prove the key swap and the router /
                            // budget model rewrites reached the upstream.
                            let auth = headers
                                .get("authorization")
                                .and_then(|v| v.to_str().ok())
                                .unwrap_or("");
                            let model = payload["model"].as_str().unwrap_or("");
                            let body = format!(
                                r#"{{"echoed_auth":"{auth}","model":"{model}","usage":{{"prompt_tokens":5,"completion_tokens":2}}}}"#
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

    /// A mock Anthropic Messages host echoing the `x-api-key` it saw, with both
    /// a non-streaming JSON body and an Anthropic-shaped SSE body.
    async fn mock_anthropic_upstream() -> String {
        let app = axum::Router::new().route(
            "/v1/messages",
            axum::routing::post(
                |headers: axum::http::HeaderMap, payload: axum::Json<Value>| async move {
                    let key = headers
                        .get("x-api-key")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    let version = headers
                        .get("anthropic-version")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    let model = payload["model"].as_str().unwrap_or("");
                    if payload["stream"].as_bool().unwrap_or(false) {
                        let body = format!(
                            "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"model\":\"{model}\",\"usage\":{{\"input_tokens\":12,\"output_tokens\":1,\"cache_read_input_tokens\":10}}}}}}\n\n\
event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"delta\":{{\"type\":\"text_delta\",\"text\":\"hi\"}}}}\n\n\
event: message_delta\ndata: {{\"type\":\"message_delta\",\"usage\":{{\"output_tokens\":7}}}}\n\n\
event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
                        );
                        axum::response::Response::builder()
                            .header("content-type", "text/event-stream")
                            .body(Body::from(body))
                            .unwrap()
                    } else {
                        let body = format!(
                            r#"{{"id":"msg_1","type":"message","model":"{model}","content":[{{"type":"text","text":"hi"}}],"usage":{{"input_tokens":12,"output_tokens":3,"cache_read_input_tokens":10}},"echoed_key":"{key}","echoed_version":"{version}"}}"#
                        );
                        axum::response::Response::builder()
                            .header("content-type", "application/json")
                            .body(Body::from(body))
                            .unwrap()
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}/v1")
    }

    async fn anthropic_state_with_budget(
        base_url: String,
        budget_store: std::sync::Arc<dyn budget::BudgetStore>,
    ) -> (std::sync::Arc<AppState>, mpsc::Receiver<TraceMessage>) {
        let (tx, rx) = mpsc::channel(64);
        use crate::providers::ProviderStore as _;
        let provider_store = std::sync::Arc::new(crate::providers::InMemoryProviderStore::default());
        // Seed an Anthropic provider that owns the Claude model.
        let tenant = uuid::Uuid::parse_str(TENANT_ID).unwrap();
        let id = provider_store
            .insert(
                tenant,
                &crate::providers::NewProvider {
                    kind: "anthropic".to_string(),
                    name: "Anthropic".to_string(),
                    base_url,
                    key: "sk-ant-upstream".to_string(),
                    metadata: crate::providers::ProviderMetadata::default(),
                    spend_limit_cents: 0,
                },
            )
            .await
            .unwrap();
        provider_store
            .record_models(id, &["claude-3-5-sonnet-20241022".to_string()])
            .await
            .unwrap();
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: "https://shared.test/v1/chat/completions".to_string(),
            tenant_store: std::sync::Arc::new(seeded_store()),
            provider_store,
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store,
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy("postgres://inferon:inferon@localhost:5432/inferon")
                .unwrap(),
        });
        (state, rx)
    }

    async fn anthropic_state(
        base_url: String,
    ) -> (std::sync::Arc<AppState>, mpsc::Receiver<TraceMessage>) {
        anthropic_state_with_budget(
            base_url,
            std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
        )
        .await
    }

    fn messages_request(body: &str, key: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("content-type", "application/json");
        if let Some(key) = key {
            builder = builder.header("x-api-key", key);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    async fn edge_request(
        body: &str,
        auth: Option<&str>,
    ) -> (axum::response::Response, mpsc::Receiver<TraceMessage>) {
        let (tx, rx) = mpsc::channel(64);
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: "http://127.0.0.1:1/v1/chat/completions".to_string(),
            tenant_store: std::sync::Arc::new(seeded_store()),
            provider_store: std::sync::Arc::new(crate::providers::InMemoryProviderStore::default()),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy("postgres://inferon:inferon@localhost:5432/inferon")
                .unwrap(),
        });
        let mut builder = Request::builder()
            .method("POST")
            .uri("/v1/edge/traces")
            .header("content-type", "application/json");
        if let Some(key) = auth {
            builder = builder.header("authorization", format!("Bearer {key}"));
        }
        let response = build_router(state)
            .oneshot(builder.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        (response, rx)
    }

    #[tokio::test]
    async fn edge_traces_are_queued_dimensioned_to_the_authenticated_tenant() {
        let body = r#"{"traces":[
            {"model":"deepseek-flash","prompt_tokens":10,"completion_tokens":2,"total_duration_ms":30,"status_code":200,"cached_prompt_tokens":8},
            {"model":"deepseek-v4-pro","prompt_tokens":5,"completion_tokens":1,"total_duration_ms":12,"status_code":200}
        ]}"#;
        let (response, mut rx) = edge_request(body, Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let json = json_body(response).await;
        assert_eq!(json["accepted"], 2);
        assert_eq!(json["dropped"], 0);
        let first = rx.recv().await.unwrap();
        assert_eq!(
            first.tenant_id,
            uuid::Uuid::parse_str(TENANT_ID).unwrap(),
            "tenant comes from the credential, never the body"
        );
        assert_eq!(first.request_model, "deepseek-flash");
        assert_eq!(first.prompt_tokens, 10);
        assert_eq!(first.cached_prompt_tokens, Some(8));
        assert_eq!(
            first.upstream_cost_usd,
            rust_decimal::Decimal::ZERO,
            "cost is recomputed in the cloud"
        );
    }

    #[tokio::test]
    async fn edge_config_requires_a_node_credential() {
        let response = build_router(test_state("http://127.0.0.1:1/v1/chat/completions"))
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/edge/config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn edge_ingest_requires_a_key() {
        let (response, _rx) = edge_request(r#"{"traces":[]}"#, None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn edge_batches_over_the_limit_are_refused() {
        let traces: Vec<Value> = (0..=crate::edge::MAX_EDGE_BATCH)
            .map(|_| json!({"model": "m", "total_duration_ms": 1, "status_code": 200}))
            .collect();
        let body = json!({"traces": traces}).to_string();
        let (response, _rx) = edge_request(&body, Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
    fn state_with_store(
        upstream_url: &str,
        tenant_store: std::sync::Arc<dyn TenantStore>,
    ) -> std::sync::Arc<AppState> {
        let (tx, _rx) = mpsc::channel::<TraceMessage>(64);
        std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: upstream_url.to_string(),
            tenant_store,
            provider_store: std::sync::Arc::new(crate::providers::InMemoryProviderStore::default()),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy(
                "postgres://inferon:inferon@localhost:5432/inferon",
            )
            .unwrap(),
        })
    }

    fn test_state(upstream_url: &str) -> std::sync::Arc<AppState> {
        state_with_store(upstream_url, std::sync::Arc::new(seeded_store()))
    }

    fn failing_state(upstream_url: &str) -> std::sync::Arc<AppState> {
        state_with_store(upstream_url, std::sync::Arc::new(FailingStore))
    }

    fn chat_request_builder(body: &str, auth: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("content-type", "application/json");
        if let Some(key) = auth {
            builder = builder.header("authorization", format!("Bearer {key}"));
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    async fn chat_request(body: &str, auth: Option<&str>) -> axum::response::Response {
        build_router(test_state(&mock_upstream().await))
            .oneshot(chat_request_builder(body, auth))
            .await
            .unwrap()
    }

    async fn traced_request(
        body: &str,
        auth: Option<&str>,
    ) -> (axum::response::Response, mpsc::Receiver<TraceMessage>) {
        traced_request_with_budget(
            body,
            auth,
            std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
        )
        .await
    }

    /// `traced_request` with a caller-supplied budget store — used to pin the
    /// ITEM-027 guard opt-out (a tenant config with guard_enabled = false).
    async fn traced_request_with_budget(
        body: &str,
        auth: Option<&str>,
        budget_store: std::sync::Arc<dyn budget::BudgetStore>,
    ) -> (axum::response::Response, mpsc::Receiver<TraceMessage>) {
        let (tx, rx) = mpsc::channel(64);
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: mock_upstream().await,
            tenant_store: std::sync::Arc::new(seeded_store()),
            provider_store: std::sync::Arc::new(crate::providers::InMemoryProviderStore::default()),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store,
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy(
                "postgres://inferon:inferon@localhost:5432/inferon",
            )
            .unwrap(),
        });
        let response = build_router(state)
            .oneshot(chat_request_builder(body, auth))
            .await
            .unwrap();
        (response, rx)
    }

    fn route_state(
        provider_store: std::sync::Arc<dyn crate::providers::ProviderStore>,
    ) -> std::sync::Arc<AppState> {
        std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: "https://shared.test/v1/chat/completions".to_string(),
            tenant_store: std::sync::Arc::new(seeded_store()),
            provider_store,
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: mpsc::channel(1).0,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy("postgres://inferon:inferon@localhost:5432/inferon")
                .unwrap(),
        })
    }

    fn seeded_tenant() -> Tenant {
        Tenant {
            id: uuid::Uuid::parse_str(TENANT_ID).unwrap(),
            upstream_openai_key: Some(MASTER_KEY.to_string()),
            api_key_id: None,
        }
    }

    #[test]
    fn surface_endpoints_join_the_base() {
        assert_eq!(
            Surface::OpenAi.endpoint("https://api.deepseek.com/v1"),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert_eq!(
            Surface::OpenAi.endpoint("https://api.deepseek.com/v1/"),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert_eq!(
            Surface::Anthropic.endpoint("https://api.anthropic.com/v1"),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[tokio::test]
    async fn anthropic_messages_are_proxied_with_the_provider_key_and_usage() {
        let base = mock_anthropic_upstream().await;
        let (state, mut rx) = anthropic_state(base).await;
        let body = r#"{"model":"claude-3-5-sonnet-20241022","max_tokens":100,"system":"Be terse.","messages":[{"role":"user","content":"hi"}]}"#;
        let response = build_router(state)
            .oneshot(messages_request(body, Some(CLIENT_KEY)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = json_body(response).await;
        assert_eq!(json["model"], "claude-3-5-sonnet-20241022");
        assert_eq!(json["usage"]["input_tokens"], 12);
        assert_eq!(
            json["echoed_key"], "sk-ant-upstream",
            "the provider key is swapped into x-api-key"
        );
        assert_eq!(json["echoed_version"], "2023-06-01");
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.request_model, "claude-3-5-sonnet-20241022");
        assert_eq!(msg.prompt_tokens, 12);
        assert_eq!(msg.completion_tokens, 3);
        assert_eq!(msg.cached_prompt_tokens, Some(10));
        assert!(
            msg.system_prompt_hash.is_some(),
            "the Anthropic `system` field is hashed"
        );
    }

    #[tokio::test]
    async fn anthropic_streaming_traces_usage_from_the_sse_frames() {
        let base = mock_anthropic_upstream().await;
        let (state, mut rx) = anthropic_state(base).await;
        let body = r#"{"model":"claude-3-5-sonnet-20241022","max_tokens":100,"stream":true,"messages":[{"role":"user","content":"hi"}]}"#;
        let response = build_router(state)
            .oneshot(messages_request(body, Some(CLIENT_KEY)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("message_stop"), "SSE passed through unchanged");
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.prompt_tokens, 12);
        assert_eq!(msg.completion_tokens, 7, "final output total from message_delta");
    }

    #[tokio::test]
    async fn anthropic_messages_without_a_key_are_401() {
        let base = mock_anthropic_upstream().await;
        let (state, _rx) = anthropic_state(base).await;
        let body = r#"{"model":"claude-3-5-sonnet-20241022","max_tokens":10,"messages":[]}"#;
        let response = build_router(state)
            .oneshot(messages_request(body, None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn anthropic_messages_without_a_provider_are_refused() {
        // A tenant with no Anthropic provider must not be hopped to the shared
        // OpenAI upstream (ITEM-050 AC5).
        let state = test_state(&mock_upstream().await);
        let body = r#"{"model":"claude-3-5-sonnet-20241022","max_tokens":10,"messages":[{"role":"user","content":"hi"}]}"#;
        let response = build_router(state)
            .oneshot(messages_request(body, Some(CLIENT_KEY)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = json_body(response).await;
        assert_eq!(json["error"]["type"], "no_provider");
    }

    #[tokio::test]
    async fn anthropic_messages_respect_the_hard_budget_cap() {
        let tenant = uuid::Uuid::parse_str(TENANT_ID).unwrap();
        let over_cap = budget::TenantBudget {
            limit_cents: 100,
            spent_cents: 100,
            cheap_fallback_model: None,
            guard_enabled: true,
            router_enabled: true,
            router_threshold: 0.35,
        };
        let store = std::sync::Arc::new(budget::InMemoryBudgetStore::new(
            [(
                tenant,
                over_cap,
            )]
            .into_iter()
            .collect(),
        ));
        let base = mock_anthropic_upstream().await;
        let (state, mut rx) = anthropic_state_with_budget(base, store).await;
        let body = r#"{"model":"claude-3-5-sonnet-20241022","max_tokens":10,"messages":[{"role":"user","content":"hi"}]}"#;
        let response = build_router(state)
            .oneshot(messages_request(body, Some(CLIENT_KEY)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.status_code, 429);
        assert_eq!(msg.error_class, Some(crate::errors::ErrorClass::Inferon4xx));
    }

    #[tokio::test]
    async fn a_warm_prefix_keeps_its_model_over_the_router() {
        // ITEM-063: the prefix is warm on deepseek-v4-pro, but the router would
        // downshift to flash; keeping the warm cache is cheaper than the miss.
        let tenant = uuid::Uuid::parse_str(TENANT_ID).unwrap();
        let system = "You are a terse assistant.";
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let affinity = std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::with_target(
            tenant,
            &crate::sha256_hex(system),
            "deepseek-v4-pro",
            now,
        ));
        let (tx, mut rx) = mpsc::channel(8);
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: mock_upstream().await,
            tenant_store: std::sync::Arc::new(seeded_store()),
            provider_store: std::sync::Arc::new(crate::providers::InMemoryProviderStore::default()),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            affinity_store: affinity,
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy("postgres://inferon:inferon@localhost:5432/inferon")
                .unwrap(),
        });
        let body = json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": "hi"}
            ]
        })
        .to_string();
        let response = build_router(state)
            .oneshot(chat_request_builder(&body, Some(CLIENT_KEY)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.request_model, "deepseek-v4-pro", "affinity kept the warm model");
        let event = msg.cache_affinity.expect("affinity event recorded");
        assert_eq!(event["from_model"], "deepseek-flash");
        assert_eq!(event["to_model"], "deepseek-v4-pro");
    }

    #[tokio::test]
    async fn an_active_policy_threshold_overrides_the_router() {
        // ITEM-064: the hot path reads the versioned policy. A threshold of 0.0
        // never downgrades, and the trace records the policy version.
        let tenant = uuid::Uuid::parse_str(TENANT_ID).unwrap();
        let policy_store = std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default());
        crate::policy::PolicyStore::publish(
            &*policy_store,
            tenant,
            crate::policy::Policy {
                version: 0,
                router_threshold: Some(0.0),
                evidence: json!({}),
                enabled: true,
                provider_weights: Default::default(),
            },
        )
        .await
        .unwrap();
        let (tx, mut rx) = mpsc::channel(8);
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: mock_upstream().await,
            tenant_store: std::sync::Arc::new(seeded_store()),
            provider_store: std::sync::Arc::new(crate::providers::InMemoryProviderStore::default()),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store,
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy("postgres://inferon:inferon@localhost:5432/inferon")
                .unwrap(),
        });
        let response = build_router(state)
            .oneshot(chat_request_builder(
                r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
                Some(CLIENT_KEY),
            ))
            .await
            .unwrap();
        let json = json_body(response).await;
        assert_eq!(json["model"], "gpt-4o", "threshold 0.0 never downgrades");
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.policy_version, Some(1));
    }

    #[tokio::test]
    async fn a_trace_records_its_proxy_overhead() {
        // ITEM-066: the proxy's own pre-upstream time is on the trace and is a
        // component of the total.
        let (response, mut rx) = traced_request(
            r#"{"model":"content","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let msg = rx.recv().await.unwrap();
        let proxy = msg.proxy_ms.expect("proxy overhead recorded");
        assert!(proxy >= 0);
        assert!(proxy <= msg.total_duration_ms, "overhead is part of the total");
    }

    #[tokio::test]
    async fn a_useful_completion_traces_ok() {
        let (response, mut rx) = traced_request(
            r#"{"model":"content","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.outcome, crate::quality::Outcome::Ok);
    }

    #[tokio::test]
    async fn a_blank_completion_traces_empty() {
        let (response, mut rx) = traced_request(
            r#"{"model":"empty","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.outcome, crate::quality::Outcome::Empty);
    }

    #[tokio::test]
    async fn a_content_filter_stop_traces_refused() {
        let (response, mut rx) = traced_request(
            r#"{"model":"refusal","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.outcome, crate::quality::Outcome::Refused);
    }

    #[tokio::test]
    async fn an_upstream_error_traces_error() {
        let (response, mut rx) = traced_request(
            r#"{"model":"fail","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.outcome, crate::quality::Outcome::Error);
    }

    #[tokio::test]
    async fn an_upstream_429_is_classified() {
        let (response, mut rx) = traced_request(
            r#"{"model":"rate","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.error_class, Some(crate::errors::ErrorClass::Upstream429));
    }

    #[tokio::test]
    async fn an_upstream_5xx_is_classified() {
        let (response, mut rx) = traced_request(
            r#"{"model":"fail","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.error_class, Some(crate::errors::ErrorClass::Upstream5xx));
    }

    #[tokio::test]
    async fn a_transport_failure_is_classified() {
        let (tx, mut rx) = mpsc::channel(8);
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: "http://127.0.0.1:1/v1/chat/completions".to_string(),
            tenant_store: std::sync::Arc::new(seeded_store()),
            provider_store: std::sync::Arc::new(crate::providers::InMemoryProviderStore::default()),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy("postgres://inferon:inferon@localhost:5432/inferon")
                .unwrap(),
        });
        let response = build_router(state)
            .oneshot(chat_request_builder(
                r#"{"model":"deepseek-flash","messages":[{"role":"user","content":"hi"}]}"#,
                Some(CLIENT_KEY),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.error_class, Some(crate::errors::ErrorClass::Transport));
    }

    #[tokio::test]
    async fn a_stream_with_content_traces_ok() {
        let (response, mut rx) = traced_request(
            r#"{"model":"streamy","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        // The trace is queued when the body is fully consumed.
        let _ = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.outcome, crate::quality::Outcome::Ok);
    }

    #[tokio::test]
    async fn a_proxied_response_carries_its_trace_id() {
        let response = chat_request(
            r#"{"model":"content","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert!(
            response.headers().get("x-inferon-trace-id").is_some(),
            "clients need the id to attach a quality signal"
        );
    }

    async fn quality_request(body: &str, auth: Option<&str>) -> axum::response::Response {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/v1/quality")
            .header("content-type", "application/json");
        if let Some(key) = auth {
            builder = builder.header("authorization", format!("Bearer {key}"));
        }
        build_router(test_state("http://127.0.0.1:1/v1/chat/completions"))
            .oneshot(builder.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_quality_signal_requires_a_key() {
        let body = format!(r#"{{"trace_id":"{}","signal":"up"}}"#, uuid::Uuid::new_v4());
        assert_eq!(
            quality_request(&body, None).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn an_unknown_quality_signal_is_refused() {
        let body = format!(r#"{{"trace_id":"{}","signal":"maybe"}}"#, uuid::Uuid::new_v4());
        let response = quality_request(&body, Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn policy_weights_pick_the_healthier_provider_for_a_model() {
        // ITEM-069: two providers own the same model; the policy's health weight
        // demotes the default (first-inserted) one, so the other is chosen.
        use crate::providers::{InMemoryProviderStore, NewProvider, ProviderStore};
        let tenant = seeded_tenant();
        let store = std::sync::Arc::new(InMemoryProviderStore::default());
        let new = |name: &str, base: &str| NewProvider {
            kind: "openai".to_string(),
            name: name.to_string(),
            base_url: base.to_string(),
            key: format!("sk-{name}"),
            metadata: Default::default(),
            spend_limit_cents: 0,
        };
        let a = store
            .insert(tenant.id, &new("a", "https://a.test/v1"))
            .await
            .unwrap();
        let b = store
            .insert(tenant.id, &new("b", "https://b.test/v1"))
            .await
            .unwrap();
        store.record_models(a, &["shared-model".to_string()]).await.unwrap();
        store.record_models(b, &["shared-model".to_string()]).await.unwrap();
        let state = route_state(store.clone());

        // Without weights, the default (first-inserted) provider wins.
        let plain = resolve_route(&state, &tenant, Some("shared-model"), Surface::OpenAi, None)
            .await
            .unwrap();
        assert_eq!(plain.provider_id, Some(a), "default ordering");

        // A weight that demotes `a` promotes `b`.
        let weights: std::collections::BTreeMap<String, f64> =
            [(a.to_string(), 0.1), (b.to_string(), 1.0)].into_iter().collect();
        let weighted = resolve_route(
            &state,
            &tenant,
            Some("shared-model"),
            Surface::OpenAi,
            Some(&weights),
        )
        .await
        .unwrap();
        assert_eq!(
            weighted.url, "https://b.test/v1/chat/completions",
            "the healthier provider wins"
        );
        assert_eq!(weighted.provider_id, Some(b));
    }

    #[tokio::test]
    async fn a_tenant_provider_overrides_the_shared_key() {
        let tenant = seeded_tenant();
        let state = route_state(std::sync::Arc::new(
            crate::providers::InMemoryProviderStore::with_provider(
                tenant.id,
                "https://my-provider.test/v1",
                "sk-my-provider",
            ),
        ));
        let route = resolve_route(&state, &tenant, None, Surface::OpenAi, None).await.expect("route");
        assert_eq!(route.url, "https://my-provider.test/v1/chat/completions");
        assert_eq!(route.key, "sk-my-provider", "the tenant's own key wins");
        assert!(route.provider_id.is_some(), "ITEM-047: provider is attributed");
    }

    #[tokio::test]
    async fn without_a_provider_the_shared_key_is_the_fallback() {
        let tenant = seeded_tenant();
        let state = route_state(std::sync::Arc::new(
            crate::providers::InMemoryProviderStore::default(),
        ));
        let route = resolve_route(&state, &tenant, None, Surface::OpenAi, None).await.expect("route");
        assert_eq!(route.url, "https://shared.test/v1/chat/completions");
        assert_eq!(route.key, MASTER_KEY);
        assert!(route.provider_id.is_none(), "shared key = no provider");
    }

    #[tokio::test]
    async fn models_endpoint_lists_the_tenants_advertised_models() {
        // ITEM-044 (live): GET /v1/models returns what the providers advertise,
        // minus disabled ones — no static catalog.
        let tenant = seeded_tenant();
        let provider_store = std::sync::Arc::new(crate::providers::InMemoryProviderStore::default());
        let provider_id = crate::providers::ProviderStore::insert(
            &*provider_store,
            tenant.id,
            &crate::providers::NewProvider {
                kind: "openai-compatible".to_string(),
                name: "host".to_string(),
                base_url: "https://host.test/v1".to_string(),
                key: "sk-host".to_string(),
                metadata: Default::default(),
                spend_limit_cents: 0,
            },
        )
        .await
        .unwrap();
        crate::providers::ProviderStore::record_models(
            &*provider_store,
            provider_id,
            &["gpt-4o".to_string(), "llama-3".to_string()],
        )
        .await
        .unwrap();
        let (tx, _rx) = mpsc::channel(8);
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: mock_upstream().await,
            tenant_store: std::sync::Arc::new(seeded_store()),
            provider_store,
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::seed(
                provider_id,
                "llama-3",
                false,
                false,
            )),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy("postgres://inferon:inferon@localhost:5432/inferon")
                .unwrap(),
        });
        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/models")
                    .header("authorization", format!("Bearer {CLIENT_KEY}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let json = json_body(response).await;
        let ids: Vec<&str> = json["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"gpt-4o"), "advertised model is listed");
        assert!(!ids.contains(&"llama-3"), "a disabled model is excluded");
    }

    #[tokio::test]
    async fn models_endpoint_falls_back_to_the_shared_upstream() {
        // No provider configured: the shared platform key serves traffic, so
        // /v1/models lists the shared upstream's models.
        let response = build_router(test_state(&mock_upstream().await))
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/models")
                    .header("authorization", format!("Bearer {CLIENT_KEY}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let json = json_body(response).await;
        assert_eq!(json["data"][0]["id"], "mock-model");
    }

    #[tokio::test]
    async fn a_tenant_without_a_provider_gets_a_no_provider_error() {
        // The platform key is not a pass-through: a keyless tenant (self-serve,
        // no provider) is told to add one instead of silently using the shared key.
        let mut by_hash = std::collections::HashMap::new();
        by_hash.insert(
            sha256_hex(CLIENT_KEY),
            Tenant {
                id: uuid::Uuid::parse_str(TENANT_ID).unwrap(),
                upstream_openai_key: None,
                api_key_id: None,
            },
        );
        let (tx, _rx) = mpsc::channel(8);
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: mock_upstream().await,
            tenant_store: std::sync::Arc::new(InMemoryTenantStore::new(by_hash)),
            provider_store: std::sync::Arc::new(crate::providers::InMemoryProviderStore::default()),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy("postgres://inferon:inferon@localhost:5432/inferon")
                .unwrap(),
        });
        let response = build_router(state)
            .oneshot(chat_request_builder(
                r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
                Some(CLIENT_KEY),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
        let json = json_body(response).await;
        assert_eq!(json["error"]["type"], "no_provider");
    }

    #[tokio::test]
    async fn a_disabled_model_is_refused_with_a_typed_error() {
        let tenant = seeded_tenant();
        let (tx, mut rx) = mpsc::channel(8);
        // Model selection is per provider: seed an openai provider and disable
        // one of its catalog models.
        let provider_store = crate::providers::InMemoryProviderStore::default();
        let provider_id = crate::providers::ProviderStore::insert(
            &provider_store,
            tenant.id,
            &crate::providers::NewProvider {
                kind: "openai".to_string(),
                name: "openai".to_string(),
                base_url: "https://my-provider.test/v1".to_string(),
                key: "sk-x".to_string(),
                metadata: Default::default(),
                spend_limit_cents: 0,
            },
        )
        .await
        .unwrap();
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: mock_upstream().await,
            tenant_store: std::sync::Arc::new(seeded_store()),
            provider_store: std::sync::Arc::new(provider_store),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::seed(
                provider_id,
                "gpt-4o",
                false,
                false,
            )),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy("postgres://inferon:inferon@localhost:5432/inferon")
                .unwrap(),
        });
        let response = build_router(state)
            .oneshot(chat_request_builder(
                "{\"model\":\"gpt-4o\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}",
                Some(CLIENT_KEY),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 64).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["type"], "model_disabled");
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.error_class, Some(crate::errors::ErrorClass::Inferon4xx));
    }

    #[tokio::test]
    async fn a_retryable_failure_falls_back_and_records_the_event() {
        let tenant = seeded_tenant();
        let (tx, mut rx) = mpsc::channel(8);
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: mock_upstream().await,
            tenant_store: std::sync::Arc::new(seeded_store()),
            provider_store: std::sync::Arc::new(
                crate::providers::InMemoryProviderStore::default(),
            ),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(
                crate::routing::InMemoryRoutingStore::with_fallbacks(tenant.id, &["deepseek-chat"]),
            ),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy("postgres://inferon:inferon@localhost:5432/inferon")
                .unwrap(),
        });
        let response = build_router(state)
            .oneshot(chat_request_builder(
                "{\"model\":\"rate\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}",
                Some(CLIENT_KEY),
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::OK,
            "the 429 primary fell back to the chain"
        );
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["model"], "deepseek-chat", "the fallback model served the request");

        let trace = rx.recv().await.expect("a trace was emitted");
        assert_eq!(trace.request_model, "deepseek-chat", "the trace records the served model");
        let event = trace.routing_event.expect("a routing event is recorded");
        assert_eq!(event["reason"], "upstream_429");
        assert_eq!(event["from_model"], "rate");
        assert_eq!(event["to_model"], "deepseek-chat");
    }

    fn dummy_trace() -> TraceMessage {
        TraceMessage {
            id: uuid::Uuid::new_v4(),
            tenant_id: uuid::Uuid::parse_str(TENANT_ID).unwrap(),
            request_model: "deepseek-chat".into(),
            system_prompt_hash: None,
            prompt_tokens: 1,
            completion_tokens: 1,
            cached_prompt_tokens: None,
            ttft_ms: Some(1),
            total_duration_ms: 1,
            status_code: 200,
            raw_prompt: None,
            raw_completion: None,
            waste_detected: None,
            upstream_cost_usd: rust_decimal::Decimal::ZERO,
            cache_savings_usd: rust_decimal::Decimal::ZERO,
            truncated: false,
            guard_event: None,
            routing_event: None,
            router_event: None,
            provider_id: None,
            api_key_id: None,
            outcome: crate::quality::Outcome::Ok,
            error_class: None,
            savings_usd: rust_decimal::Decimal::ZERO,
            savings_kind: None,
            cache_affinity: None,
            policy_version: None,
            proxy_ms: None,
        }
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

    /// State with a budget configuration for the seeded tenant.
    fn budgeted_state(
        upstream_url: &str,
        budget: budget::TenantBudget,
    ) -> std::sync::Arc<AppState> {
        let mut budgets = std::collections::HashMap::new();
        budgets.insert(uuid::Uuid::parse_str(TENANT_ID).unwrap(), budget);
        let (tx, _rx) = mpsc::channel::<TraceMessage>(64);
        std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: upstream_url.to_string(),
            tenant_store: std::sync::Arc::new(seeded_store()),
            provider_store: std::sync::Arc::new(crate::providers::InMemoryProviderStore::default()),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::new(budgets)),
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy(
                "postgres://inferon:inferon@localhost:5432/inferon",
            )
            .unwrap(),
        })
    }

    /// Same state as `state` but with a fresh telemetry channel so the caller
    /// can observe emitted traces.
    fn with_trace_channel(
        state: &std::sync::Arc<AppState>,
    ) -> (std::sync::Arc<AppState>, mpsc::Receiver<TraceMessage>) {
        let (tx, rx) = mpsc::channel(64);
        let state = std::sync::Arc::new(AppState {
            http_client: state.http_client.clone(),
            upstream_url: state.upstream_url.clone(),
            tenant_store: state.tenant_store.clone(),
            provider_store: std::sync::Arc::new(crate::providers::InMemoryProviderStore::default()),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store: state.budget_store.clone(),
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            operator_key: state.operator_key.clone(),
            accounts: None,
            db: state.db.clone(),
            telemetry_tx: tx,
        });
        (state, rx)
    }

    async fn json_body(response: axum::response::Response) -> Value {
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn a_traced_request_is_attributed_to_its_api_key() {
        // ITEM-047: the authenticated key id lands on the trace so the dashboard
        // can break spend down per key.
        let (response, mut rx) = traced_request(
            r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let msg = rx.recv().await.unwrap();
        assert!(msg.api_key_id.is_some(), "trace is attributed to the api key");
        assert!(msg.provider_id.is_none(), "shared key means no provider");
    }

    #[tokio::test]
    async fn simple_gpt4o_request_is_downgraded_to_mini() {
        let (response, mut rx) = traced_request(
            r#"{"model":"gpt-4o","messages":[{"role":"user","content":"write a haiku"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let json = json_body(response).await;
        assert_eq!(json["model"], "gpt-4o-mini", "upstream must see the downgraded model");
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.request_model, "gpt-4o-mini", "trace reflects the actual model");
    }

    #[tokio::test]
    async fn simple_deepseek_v4_pro_is_downgraded_and_traced() {
        // ITEM-054: the router now downshifts DeepSeek-native traffic too, and
        // the decision is recorded on the trace.
        let (response, mut rx) = traced_request(
            r#"{"model":"deepseek-v4-pro","messages":[{"role":"user","content":"write a haiku"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let json = json_body(response).await;
        assert_eq!(json["model"], "deepseek-flash", "upstream sees the cheap tier");
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.request_model, "deepseek-flash");
        let event = msg.router_event.expect("router event recorded");
        assert_eq!(event["from_model"], "deepseek-v4-pro");
        assert_eq!(event["to_model"], "deepseek-flash");
        assert_eq!(event["reason"], "heuristic_router");
    }

    #[tokio::test]
    async fn disabling_the_router_keeps_the_client_model() {
        // ITEM-046 AC3: the controls are read per request, so an update applies
        // without a redeploy — flipping the router off stops the downgrade.
        let base = budgeted_state(
            &mock_upstream().await,
            budget::TenantBudget {
                limit_cents: 0,
                spent_cents: 0,
                cheap_fallback_model: None,
                guard_enabled: true,
                router_enabled: true,
                router_threshold: 0.35,
            },
        );
        // Simple gpt-4o → downgraded while the router is on.
        let response = build_router(base.clone())
            .oneshot(chat_request_builder(
                r#"{"model":"gpt-4o","messages":[{"role":"user","content":"write a haiku"}]}"#,
                Some(CLIENT_KEY),
            ))
            .await
            .unwrap();
        assert_eq!(json_body(response).await["model"], "gpt-4o-mini");

        // The tenant turns the router off (no redeploy) → the client model stands.
        base.budget_store
            .update_config(
                uuid::Uuid::parse_str(TENANT_ID).unwrap(),
                &budget::ConfigUpdate {
                    budget_limit_cents: 0,
                    cheap_fallback_model: None,
                    guard_enabled: true,
                    router_enabled: false,
                    router_threshold: 0.35,
                },
            )
            .await
            .unwrap();
        let response = build_router(base)
            .oneshot(chat_request_builder(
                r#"{"model":"gpt-4o","messages":[{"role":"user","content":"write a haiku"}]}"#,
                Some(CLIENT_KEY),
            ))
            .await
            .unwrap();
        assert_eq!(
            json_body(response).await["model"],
            "gpt-4o",
            "router off keeps the request model"
        );
    }

    #[tokio::test]
    async fn tool_using_gpt4o_is_not_downgraded() {
        let (response, _rx) = traced_request(
            r#"{"model":"gpt-4o","tools":[{"type":"function"}],"messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let json = json_body(response).await;
        assert_eq!(json["model"], "gpt-4o", "complex requests keep the client model");
    }

    #[tokio::test]
    async fn deepseek_requests_are_never_rewritten() {
        let (response, mut rx) = traced_request(
            r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let json = json_body(response).await;
        assert_eq!(json["model"], "deepseek-chat");
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.request_model, "deepseek-chat");
    }

    #[tokio::test]
    async fn hard_cap_budget_rejects_with_429_and_traces() {
        let base = budgeted_state(
            &mock_upstream().await,
            budget::TenantBudget {
                limit_cents: 1000,
                spent_cents: 1000,
                cheap_fallback_model: Some("gpt-4o-mini".to_string()),
            guard_enabled: true,
            router_enabled: true,
            router_threshold: 0.35,
            },
        );
        let (state, mut rx) = with_trace_channel(&base);
        let response = build_router(state)
            .oneshot(chat_request_builder(
                r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
                Some(CLIENT_KEY),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.status_code, 429);
    }

    #[tokio::test]
    async fn near_cap_budget_downgrades_to_cheap_model() {
        let state = budgeted_state(
            &mock_upstream().await,
            budget::TenantBudget {
                limit_cents: 1000,
                spent_cents: 900, // in the last 15% zone
                cheap_fallback_model: Some("gpt-4o-mini".to_string()),
            guard_enabled: true,
            router_enabled: true,
            router_threshold: 0.35,
            },
        );
        let response = build_router(state)
            .oneshot(chat_request_builder(
                r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
                Some(CLIENT_KEY),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let json = json_body(response).await;
        assert_eq!(json["model"], "gpt-4o-mini", "budget fallback rewrites the model");
    }

    #[tokio::test]
    async fn within_budget_requests_forward_unchanged() {
        let state = budgeted_state(
            &mock_upstream().await,
            budget::TenantBudget {
                limit_cents: 1000,
                spent_cents: 100,
                cheap_fallback_model: Some("gpt-4o-mini".to_string()),
            guard_enabled: true,
            router_enabled: true,
            router_threshold: 0.35,
            },
        );
        let response = build_router(state)
            .oneshot(chat_request_builder(
                r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
                Some(CLIENT_KEY),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let json = json_body(response).await;
        assert_eq!(json["model"], "deepseek-chat");
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
    async fn non_json_body_returns_400() {
        let response = chat_request("this is not json", Some(CLIENT_KEY)).await;

        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    // --- ITEM-034: api keys as rows (multi-key + revoke) ---

    const SECOND_KEY: &str = "sk-client-dev-key-2";
    /// `seeded_store` plus a second active key (SECOND_KEY) for the same tenant.
    async fn store_with_second_key() -> (InMemoryTenantStore, uuid::Uuid) {
        let store = seeded_store();
        let id = store
            .insert_api_key(
                uuid::Uuid::parse_str(TENANT_ID).unwrap(),
                "ci",
                &sha256_hex(SECOND_KEY),
            )
            .await
            .expect("mint succeeds");
        (store, id)
    }

    #[tokio::test]
    async fn insert_tenant_creates_a_tenant_keys_can_attach_to() {
        // ITEM-036: self-serve provisioning mints a tenant row, then attaches
        // an owner membership and a first key through the ITEM-034 path.
        let store = InMemoryTenantStore::default();
        let upstream = store.insert_tenant(Some("sk-upstream")).await.expect("ok");
        let tenant = store.tenant_by_id(upstream).await.expect("ok").expect("tenant present");
        assert_eq!(tenant.upstream_openai_key.as_deref(), Some("sk-upstream"));

        let keyless = store.insert_tenant(None).await.expect("ok");
        assert!(store.tenant_by_id(keyless).await.expect("ok").unwrap().upstream_openai_key.is_none());

        // A minted key on the provisioned tenant resolves back to it.
        let hash = sha256_hex("sk-provisioned-dev");
        store.insert_api_key(upstream, "default", &hash).await.expect("key minted");
        let resolved = store.tenant_by_api_key_hash(&hash).await.expect("ok");
        assert_eq!(resolved.map(|t| t.id), Some(upstream));
    }

    #[tokio::test]
    async fn revoke_tenant_key_only_touches_the_owners_keys() {
        // ITEM-038 AC2: a tenant-scoped revoke can never kill another tenant's
        // key (tenant boundaries hold even with a known key id).
        let store = InMemoryTenantStore::default();
        let tenant_a = store.insert_tenant(Some("a")).await.expect("ok");
        let tenant_b = store.insert_tenant(Some("b")).await.expect("ok");
        let key_a = store
            .insert_api_key(tenant_a, "a-key", "hash-a")
            .await
            .expect("ok");
        let key_b = store
            .insert_api_key(tenant_b, "b-key", "hash-b")
            .await
            .expect("ok");
        // Tenant A cannot revoke B's key…
        assert!(!store.revoke_tenant_key(tenant_a, key_b).await.expect("ok"));
        // …and B's key still authenticates.
        assert!(store.tenant_by_api_key_hash("hash-b").await.expect("ok").is_some());
        // Tenant A revokes its own key; revoking again is a no-op.
        assert!(store.revoke_tenant_key(tenant_a, key_a).await.expect("ok"));
        assert!(!store.revoke_tenant_key(tenant_a, key_a).await.expect("ok"));
        assert!(store.tenant_by_api_key_hash("hash-a").await.expect("ok").is_none());
    }

    #[tokio::test]
    async fn a_second_active_key_authenticates_to_the_same_tenant() {
        let (store, _) = store_with_second_key().await;
        let tenant = store
            .tenant_by_api_key_hash(&sha256_hex(SECOND_KEY))
            .await
            .expect("store ok")
            .expect("second key resolves");
        assert_eq!(tenant.id.to_string(), TENANT_ID);
        // Both keys share the tenant's upstream key.
        assert_eq!(tenant.upstream_openai_key.as_deref(), Some(MASTER_KEY));
    }

    #[tokio::test]
    async fn revoking_a_key_stops_it_authenticating_immediately() {        let (store, second_key_id) = store_with_second_key().await;
        assert!(store
            .revoke_api_key(second_key_id)
            .await
            .expect("revoke ok"), "revoked a live key");
        assert_eq!(
            store
                .tenant_by_api_key_hash(&sha256_hex(SECOND_KEY))
                .await
                .expect("store ok"),
            None,
            "revoked key resolves to nothing"
        );
        // The original key is untouched.
        assert!(store
            .tenant_by_api_key_hash(&sha256_hex(CLIENT_KEY))
            .await
            .expect("store ok")
            .is_some());
        // Revoking again (or an unknown id) reports no-op.
        assert!(!store
            .revoke_api_key(second_key_id)
            .await
            .expect("revoke ok"), "already revoked");
        assert!(!store
            .revoke_api_key(uuid::Uuid::new_v4())
            .await
            .expect("revoke ok"), "unknown key id");
    }

    #[tokio::test]
    async fn key_mint_rejects_unknown_tenants_and_list_reports_revocation() {
        let store = seeded_store();
        assert!(store
            .insert_api_key(uuid::Uuid::new_v4(), "ci", &sha256_hex("sk-x"))
            .await
            .is_err(), "minting for a missing tenant fails");
        let (store, second_key_id) = store_with_second_key().await;
        let tenant_id = uuid::Uuid::parse_str(TENANT_ID).unwrap();
        let listed = store.list_api_keys(tenant_id).await.expect("list ok");
        assert_eq!(listed.len(), 2, "seeded + minted key");
        assert!(listed.iter().any(|k| k.label == "ci" && !k.revoked()));
        store.revoke_api_key(second_key_id).await.expect("revoke ok");
        let listed = store.list_api_keys(tenant_id).await.expect("list ok");
        assert!(listed.iter().any(|k| k.label == "ci" && k.revoked()));
        assert_eq!(
            store.list_api_keys(uuid::Uuid::new_v4()).await.expect("list ok").len(),
            0,
            "no keys for other tenants"
        );
    }

    /// Proxy-level check: chat request against a custom store + custom key.
    async fn chat_request_with_store(body: &str, key: &str, store: InMemoryTenantStore) -> axum::response::Response {
        let state = state_with_store(
            &mock_upstream().await,
            std::sync::Arc::new(store),
        );
        build_router(state)
            .oneshot(chat_request_builder(body, Some(key)))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn revoked_key_returns_401_while_other_keys_keep_working() {
        let body = r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#;
        // Second active key works end-to-end.
        let (store, _) = store_with_second_key().await;
        let ok = chat_request_with_store(body, SECOND_KEY, store).await;
        assert_eq!(ok.status(), axum::http::StatusCode::OK);

        // After revocation the same key is 401; the original key still works.
        let (store, second_key_id) = store_with_second_key().await;
        store.revoke_api_key(second_key_id).await.expect("revoke ok");
        let denied = chat_request_with_store(body, SECOND_KEY, store.clone()).await;
        assert_eq!(denied.status(), axum::http::StatusCode::UNAUTHORIZED);
        let ok2 = chat_request_with_store(body, CLIENT_KEY, store).await;
        assert_eq!(ok2.status(), axum::http::StatusCode::OK);
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

    // --- ITEM-005 telemetry ---

    #[test]
    fn usage_from_sse_bytes_parses_final_usage_frame() {
        assert_eq!(
            usage_from_sse_bytes(SSE_BODY.as_bytes()),
            Some(Usage {
                prompt_tokens: 5,
                completion_tokens: 2,
                cached_tokens: None,
            })
        );
    }

    #[test]
    fn usage_from_sse_bytes_handles_frames_split_across_chunks() {
        let mut acc = Vec::new();
        for chunk in SSE_BODY.as_bytes().chunks(7) {
            acc.extend_from_slice(chunk);
        }
        assert_eq!(
            usage_from_sse_bytes(&acc),
            Some(Usage {
                prompt_tokens: 5,
                completion_tokens: 2,
                cached_tokens: None,
            })
        );
    }

    #[test]
    fn usage_from_sse_bytes_returns_none_when_absent() {
        assert_eq!(usage_from_sse_bytes(b"data: {\"a\":1}\n\ndata: [DONE]\n\n"), None);
        assert_eq!(usage_from_sse_bytes(b""), None);
        assert_eq!(usage_from_sse_bytes(b"not sse at all"), None);
    }

    #[test]
    fn usage_from_sse_bytes_reads_cache_evidence() {
        let sse = "data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2,\"prompt_tokens_details\":{\"cached_tokens\":7}}}\n\n";
        assert_eq!(
            usage_from_sse_bytes(sse.as_bytes()),
            Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 2,
                cached_tokens: Some(7),
            })
        );
    }

    #[test]
    fn system_prompt_hash_hashes_first_system_message() {
        let payload: serde_json::Value = serde_json::json!({
            "messages": [
                {"role": "system", "content": "be terse"},
                {"role": "system", "content": "second"},
                {"role": "user", "content": "hi"}
            ]
        });
        assert_eq!(
            system_prompt_hash(&payload).as_deref(),
            Some(sha256_hex("be terse").as_str())
        );
    }

    #[test]
    fn system_prompt_hash_is_none_without_system_message() {
        let payload: serde_json::Value =
            serde_json::json!({"messages": [{"role": "user", "content": "hi"}]});
        assert_eq!(system_prompt_hash(&payload), None);
        assert_eq!(system_prompt_hash(&serde_json::json!({})), None);
    }

    #[tokio::test]
    async fn non_stream_request_emits_trace_with_tokens_and_raw() {
        let (response, mut rx) = traced_request(
            r#"{"model":"deepseek-chat","messages":[{"role":"system","content":"be terse"},{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let msg = rx.recv().await.expect("trace should be emitted");
        assert_eq!(msg.tenant_id, uuid::Uuid::parse_str(TENANT_ID).unwrap());
        assert_eq!(msg.request_model, "deepseek-chat");
        assert_eq!(msg.prompt_tokens, 5);
        assert_eq!(msg.completion_tokens, 2);
        assert_eq!(msg.status_code, 200);
        assert_eq!(
            msg.system_prompt_hash.as_deref(),
            Some(sha256_hex("be terse").as_str())
        );
        assert_eq!(
            msg.raw_prompt.as_ref().unwrap()["messages"][0]["role"],
            "system"
        );
        assert_eq!(
            msg.raw_completion.as_ref().unwrap()["usage"]["prompt_tokens"],
            5
        );
    }

    #[tokio::test]
    async fn non_stream_without_system_message_has_null_hash_and_ttft() {
        let (response, mut rx) = traced_request(
            r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let msg = rx.recv().await.expect("trace should be emitted");
        assert_eq!(msg.system_prompt_hash, None);
        assert!(msg.ttft_ms.is_some(), "non-stream ttft mirrors total duration");
        assert!(msg.total_duration_ms >= 0);
        assert_eq!(msg.prompt_tokens, 5);
    }

    #[tokio::test]
    async fn stream_request_emits_trace_with_usage_from_final_chunk() {
        let (response, mut rx) = traced_request(
            r#"{"model":"deepseek-chat","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), SSE_BODY.as_bytes());
        let msg = rx
            .recv()
            .await
            .expect("trace emitted after the stream body is consumed");
        assert_eq!(msg.prompt_tokens, 5, "tokens parsed from final chunk");
        assert_eq!(msg.completion_tokens, 2);
        assert!(msg.ttft_ms.is_some(), "ttft recorded at first chunk");
        assert!(msg.total_duration_ms >= 0);
        assert_eq!(msg.status_code, 200);
        assert_eq!(msg.raw_completion, None, "stream raw deferred (per spec)");
    }

    #[tokio::test]
    async fn stream_without_usage_frame_records_zero_tokens() {
        let (response, mut rx) = traced_request(
            r#"{"model":"nousage","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let _body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let msg = rx.recv().await.expect("trace should be emitted");
        assert_eq!(msg.prompt_tokens, 0);
        assert_eq!(msg.completion_tokens, 0);
    }

    #[tokio::test]
    async fn upstream_error_response_is_traced_with_status() {
        let (response, mut rx) = traced_request(
            r#"{"model":"rate","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        let _body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let msg = rx.recv().await.expect("trace should be emitted");
        assert_eq!(msg.status_code, 429);
        assert_eq!(msg.prompt_tokens, 0);
    }

    #[tokio::test]
    async fn telemetry_send_never_blocks_when_channel_full() {
        let (tx, rx_keepalive) = mpsc::channel::<TraceMessage>(1);
        // Keep the receiver alive but never drain it: the channel fills after
        // the first request's trace. Later requests must still complete (AC5).
        let _keepalive = rx_keepalive;
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: mock_upstream().await,
            tenant_store: std::sync::Arc::new(seeded_store()),
            provider_store: std::sync::Arc::new(crate::providers::InMemoryProviderStore::default()),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy(
                "postgres://inferon:inferon@localhost:5432/inferon",
            )
            .unwrap(),
        });

        let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            for _ in 0..3 {
                let response = build_router(state.clone())
                    .oneshot(chat_request_builder(
                        r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
                        Some(CLIENT_KEY),
                    ))
                    .await
                    .unwrap();
                assert_eq!(response.status(), axum::http::StatusCode::OK);
                let _ = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap();
            }
        })
        .await;
        assert!(
            result.is_ok(),
            "requests must not block on a full telemetry channel"
        );
    }

    // Worker batching policy — sink-agnostic (the Postgres sink is exercised at
    // the deployment seam where a real database exists).
    #[derive(Default)]
    struct SinkCounters {
        flushes: usize,
        messages: usize,
        cache_miss_flags: usize,
        over_provisioned_flags: usize,
        savings_usd: rust_decimal::Decimal,
        router_savings: usize,
    }

    struct SharedSink(std::sync::Arc<tokio::sync::Mutex<SinkCounters>>);

    #[async_trait::async_trait]
    impl TraceSink for SharedSink {
        async fn flush(&mut self, batch: &mut Vec<TraceMessage>) {
            let mut counters = self.0.lock().await;
            counters.flushes += 1;
            for msg in batch.drain(..) {
                counters.messages += 1;
                if let Some(waste) = &msg.waste_detected {
                    if waste["cache_miss_system_prompt"] == true {
                        counters.cache_miss_flags += 1;
                    }
                    if waste["over_provisioned"] == true {
                        counters.over_provisioned_flags += 1;
                    }
                }
                counters.savings_usd += msg.savings_usd;
                if msg.savings_kind.as_deref() == Some("router") {
                    counters.router_savings += 1;
                }
            }
        }
    }

    fn spawn_worker(
        rx: mpsc::Receiver<TraceMessage>,
        flush_interval_ms: u64,
        batch_size: usize,
    ) -> std::sync::Arc<tokio::sync::Mutex<SinkCounters>> {
        spawn_worker_with_counts(rx, flush_interval_ms, batch_size, HashMap::new())
    }

    fn spawn_worker_with_counts(
        rx: mpsc::Receiver<TraceMessage>,
        flush_interval_ms: u64,
        batch_size: usize,
        initial_counts: HashMap<String, u32>,
    ) -> std::sync::Arc<tokio::sync::Mutex<SinkCounters>> {
        let counters = std::sync::Arc::new(tokio::sync::Mutex::new(SinkCounters::default()));
        let sink: Box<dyn TraceSink> = Box::new(SharedSink(counters.clone()));
        tokio::spawn(telemetry_worker(
            rx,
            sink,
            std::time::Duration::from_millis(flush_interval_ms),
            batch_size,
            initial_counts,
        ));
        counters
    }

    async fn wait_until(
        counters: std::sync::Arc<tokio::sync::Mutex<SinkCounters>>,
        predicate: impl Fn(&SinkCounters) -> bool,
    ) -> bool {
        for _ in 0..200 {
            if predicate(&*counters.lock().await) {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        false
    }

    #[tokio::test]
    async fn worker_attributes_router_savings() {
        let (tx, rx) = mpsc::channel(8);
        let counters = spawn_worker(rx, 50, 16);
        let mut msg = dummy_trace();
        // Router downgraded gpt-4o -> gpt-4o-mini for a 1M-token prompt:
        // 2.50 - 0.15 = 2.35 saved.
        msg.request_model = "gpt-4o-mini".to_string();
        msg.prompt_tokens = 1_000_000;
        msg.completion_tokens = 0;
        msg.router_event = Some(json!({
            "from_model": "gpt-4o",
            "to_model": "gpt-4o-mini",
            "reason": "heuristic_router"
        }));
        tx.send(msg).await.unwrap();
        assert!(wait_until(counters.clone(), |c| c.messages == 1).await);
        let counters = counters.lock().await;
        assert_eq!(counters.savings_usd.to_string(), "2.35");
        assert_eq!(counters.router_savings, 1);
    }

    #[tokio::test]
    async fn worker_flushes_when_batch_threshold_reached() {
        let (tx, rx) = mpsc::channel(256);
        let counters = spawn_worker(rx, 60_000, 5); // interval never fires in this test
        for _ in 0..5 {
            tx.send(dummy_trace()).await.unwrap();
        }
        assert!(
            wait_until(counters.clone(), |c| c.messages >= 5).await,
            "worker should flush once 5 messages accumulate"
        );
        let counters = counters.lock().await;
        assert_eq!(counters.messages, 5);
        assert_eq!(counters.flushes, 1, "one transactional flush per batch");
    }

    #[tokio::test]
    async fn worker_honors_counts_persisted_before_restart() {
        // A restart reloads counts from the DB (initial_counts). A prompt seen
        // once before the restart must flag on its first post-restart hit.
        let (tx, rx) = mpsc::channel(256);
        let mut persisted = HashMap::new();
        persisted.insert("hash-repeated".to_string(), 1);
        let counters = spawn_worker_with_counts(rx, 60_000, 100, persisted);
        let mut msg = dummy_trace();
        msg.system_prompt_hash = Some("hash-repeated".to_string());
        msg.prompt_tokens = 5000;
        msg.cached_prompt_tokens = Some(0);
        tx.send(msg).await.unwrap();
        drop(tx);
        assert!(
            wait_until(counters.clone(), |c| c.cache_miss_flags >= 1).await,
            "second-ever occurrence must be flagged even across a restart"
        );
    }

    #[tokio::test]
    async fn worker_flushes_on_interval_when_idle() {
        let (tx, rx) = mpsc::channel(256);
        let counters = spawn_worker(rx, 40, 100);
        tx.send(dummy_trace()).await.unwrap();
        assert!(
            wait_until(counters.clone(), |c| c.messages >= 1).await,
            "worker should flush on the interval even below the batch size"
        );
        drop(tx);
    }

    #[tokio::test]
    async fn worker_flushes_remainder_and_exits_when_senders_drop() {
        let (tx, rx) = mpsc::channel(256);
        let counters = spawn_worker(rx, 60_000, 100);
        tx.send(dummy_trace()).await.unwrap();
        tx.send(dummy_trace()).await.unwrap();
        drop(tx); // all senders gone -> worker drains the remainder and exits
        assert!(
            wait_until(counters.clone(), |c| c.messages >= 2).await,
            "worker should flush the remainder on channel close"
        );
    }

    /// Drive a real HTTP request through axum::serve and close the socket
    /// after the first SSE frame — a genuine client abort mid-stream.
    async fn serve_and_abort(model: &str) -> mpsc::Receiver<TraceMessage> {
        let (tx, rx) = mpsc::channel(8);
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: mock_upstream().await,
            tenant_store: std::sync::Arc::new(seeded_store()),
            provider_store: std::sync::Arc::new(crate::providers::InMemoryProviderStore::default()),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy(
                "postgres://inferon:inferon@localhost:5432/inferon",
            )
            .unwrap(),
        });
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, build_router(state)).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let body = format!(
            r#"{{"model":"{model}","stream":true,"messages":[{{"role":"user","content":"hi"}}]}}"#
        );
        let request = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAuthorization: Bearer {CLIENT_KEY}\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        socket.write_all(request.as_bytes()).await.unwrap();
        // Read until at least the first SSE frame has arrived, then hang for a
        // moment inside the mock's 300ms gap and drop the connection.
        let mut buf = [0u8; 2048];
        let mut seen = 0usize;
        while seen < buf.len() {
            let n = socket.read(&mut buf[seen..]).await.unwrap();
            if n == 0 {
                break;
            }
            seen += n;
            if String::from_utf8_lossy(&buf[..seen]).contains("content\":\"a\"") {
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        drop(socket); // abort
        rx
    }

    #[tokio::test]
    async fn client_abort_mid_stream_records_single_truncated_trace() {
        let mut rx = serve_and_abort("slow").await;
        let msg = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
            .await
            .expect("truncated trace should be emitted")
            .expect("channel open");
        assert!(msg.truncated, "aborted stream must be flagged truncated");
        assert_eq!(msg.status_code, 200);
        assert_eq!(msg.error_class, Some(crate::errors::ErrorClass::ClientAbort));
        assert!(msg.raw_completion.is_none());
        // Exactly one trace: the abort must not also fire the normal emit.
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        assert!(rx.try_recv().is_err(), "no duplicate trace after abort");
    }

    #[tokio::test]
    async fn upstream_mid_stream_error_records_truncated_trace() {
        let (tx, mut rx) = mpsc::channel(8);
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: mock_upstream().await,
            tenant_store: std::sync::Arc::new(seeded_store()),
            provider_store: std::sync::Arc::new(crate::providers::InMemoryProviderStore::default()),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy(
                "postgres://inferon:inferon@localhost:5432/inferon",
            )
            .unwrap(),
        });
        let response = build_router(state)
            .oneshot(chat_request_builder(
                r#"{"model":"miderror","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
                Some(CLIENT_KEY),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&body).contains("content\":\"a\""),
            "client still receives the frames before the failure"
        );
        let msg = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
            .await
            .expect("trace should be emitted")
            .expect("channel open");
        assert!(msg.truncated, "mid-stream upstream error must be flagged truncated");
    }

    #[tokio::test]
    async fn worker_lints_repeated_uncached_system_prompts_off_the_hot_path() {
        let (tx, rx) = mpsc::channel(256);
        let counters = spawn_worker(rx, 60_000, 100);
        // Two identical traces: same hash, >=4000 prompt tokens, no cache benefit.
        for _ in 0..2 {
            let mut msg = dummy_trace();
            msg.system_prompt_hash = Some("hash-repeated".to_string());
            msg.prompt_tokens = 5000;
            msg.cached_prompt_tokens = Some(0);
            tx.send(msg).await.unwrap();
        }
        drop(tx);
        assert!(
            wait_until(counters.clone(), |c| c.cache_miss_flags >= 1).await,
            "second occurrence of a large uncached system prompt must be flagged"
        );
    }

    // --- ITEM-023: schema guard buffered validation seam ---

    /// A streamed request carrying a JSON-schema contract and a cheap-tier
    /// model (gpt-4o-mini) — the guard's trigger condition.
    fn guarded_stream_body(model: &str, marker: &str) -> String {
        serde_json::json!({
            "model": model,
            "stream": true,
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "answer",
                    "schema": {"type": "object", "properties": {"answer": {"type": "string"}}, "required": ["answer"]}
                }
            },
            "messages": [{"role": "user", "content": marker}]
        })
        .to_string()
    }

    #[tokio::test]
    async fn guarded_valid_stream_buffers_then_flushes_after_done() {
        // gpt-4o-mini + schema contract + stream -> guarded path. The mock's
        // generic streaming route returns a well-formed SSE body; the client
        // must receive it whole (buffered, then flushed after [DONE]).
        let (response, mut rx) =
            traced_request(&guarded_stream_body("gpt-4o-mini", "valid-json"), Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/event-stream"
        );
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let expected = delta_only_stream(r#"{"answer":"42"}"#);
        assert_eq!(
            body.as_ref(),
            expected.as_bytes(),
            "buffered schema-valid body flushed whole after [DONE]"
        );
        let msg = rx.recv().await.expect("trace emitted");
        assert_eq!(msg.status_code, 200);
        assert!(!msg.truncated, "clean guarded flush is not truncated");
        // 026 AC1: every guard event records outcome on the trace — a clean
        // cheap flush is outcome "flushed", no failure reason.
        let ev = msg.guard_event.as_ref().expect("guard event recorded on flush");
        assert_eq!(ev["outcome"], "flushed");
        assert_eq!(ev["guarded_model"], "gpt-4o-mini");
        assert!(ev.get("reason").is_none(), "no failure reason on a clean flush");
    }

    #[tokio::test]
    async fn guarded_schema_violation_falls_back_to_the_frontier() {
        // 025 AC1: the cheap model streams well-formed JSON that violates the
        // answer schema ("wrong-shape" -> {"other":1}). The bad buffer is
        // discarded and the original prompt replayed on the tenant's frontier
        // (deepseek-v4-pro — a cheap original escalates to the deployment
        // frontier). The client receives one valid 200 SSE response and one
        // trace carrying the guard event (AC2/AC3).
        let (response, mut rx) =
            traced_request(&guarded_stream_body("gpt-4o-mini", "wrong-shape"), Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/event-stream"
        );
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let expected = delta_only_stream(r#"{"answer":"42"}"#);
        assert_eq!(
            body.as_ref(),
            expected.as_bytes(),
            "frontier replay flushed whole — never a cheap partial body"
        );
        let msg = rx.recv().await.expect("exactly one trace");
        assert_eq!(msg.status_code, 200);
        assert!(!msg.truncated, "clean fallback is not truncated");
        assert_eq!(
            msg.request_model, "deepseek-v4-pro",
            "trace model = the model that ultimately served (cost basis)"
        );
        let ev = msg.guard_event.as_ref().expect("guard event recorded");
        assert_eq!(ev["outcome"], "fallback");
        assert_eq!(ev["reason"], "schema_violation");
        assert_eq!(ev["guarded_model"], "gpt-4o-mini");
        assert!(rx.try_recv().is_err(), "fallback is not a second request");
    }

    #[tokio::test]
    async fn guarded_truncated_upstream_falls_back_to_the_frontier() {
        // Container failure (JSON frames but no [DONE]) is a classified
        // UpstreamFailure: the buffer is discarded and the request falls back
        // instead of showing the client a truncated cheap body.
        let (response, mut rx) =
            traced_request(&guarded_stream_body("gpt-4o-mini", "truncated"), Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let expected = delta_only_stream(r#"{"answer":"42"}"#);
        assert_eq!(body.as_ref(), expected.as_bytes());
        let msg = rx.recv().await.expect("exactly one trace");
        assert!(!msg.truncated);
        let ev = msg.guard_event.as_ref().expect("guard event recorded");
        assert_eq!(ev["outcome"], "fallback");
        assert_eq!(ev["reason"], "upstream_failure");
        assert_eq!(msg.request_model, "deepseek-v4-pro");
    }

    #[tokio::test]
    async fn downgraded_frontier_request_falls_back_to_the_original_model() {
        // The tenant asked gpt-4o; the router downgraded it to gpt-4o-mini and
        // the cheap response violated the schema. The replay must target the
        // ORIGINALLY REQUESTED model (gpt-4o), not the deployment default.
        let (response, mut rx) =
            traced_request(&guarded_stream_body("gpt-4o", "wrong-shape"), Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let expected = delta_only_stream(r#"{"answer":"42"}"#);
        assert_eq!(body.as_ref(), expected.as_bytes());
        let msg = rx.recv().await.expect("exactly one trace");
        assert_eq!(msg.request_model, "gpt-4o", "replayed to the original model");
        let ev = msg.guard_event.as_ref().expect("guard event recorded");
        assert_eq!(ev["guarded_model"], "gpt-4o-mini", "the guard fired on the downgrade");
        assert_eq!(ev["reason"], "schema_violation");
    }

    #[tokio::test]
    async fn guarded_fallback_upstream_failure_is_loud_and_clean() {
        // 025 AC4: the frontier replay itself fails (mock 500 for model
        // deepseek-v4-pro on marker "boom"). The client gets one clean 502 —
        // no silent data loss — and the single trace preserves the guard
        // reason plus the fallback failure detail.
        let (response, mut rx) =
            traced_request(&guarded_stream_body("gpt-4o-mini", "boom"), Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), axum::http::StatusCode::BAD_GATEWAY);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["type"], "guard_fallback_error");
        let msg = rx.recv().await.expect("exactly one trace");
        assert_eq!(msg.status_code, 200, "answered (not truncated)");
        assert_eq!(msg.request_model, "deepseek-v4-pro");
        let ev = msg.guard_event.as_ref().expect("guard event recorded");
        assert_eq!(ev["outcome"], "fallback_failed");
        assert_eq!(ev["reason"], "upstream_failure", "the original guard reason");
        assert_eq!(ev["detail"], "http 500");
        assert!(rx.try_recv().is_err(), "still exactly one trace");
    }

    #[tokio::test]
    async fn tenant_guard_opt_out_restores_byte_for_byte_passthrough() {
        // 027 AC1: with tenant_configurations.guard_enabled = false the lying
        // cheap stream pipes through unchanged (no buffering, no fallback) and
        // no guard event is recorded — the opt-out is per tenant, everyone
        // else keeps the default-on guard.
        let tenant_id = uuid::Uuid::parse_str(TENANT_ID).unwrap();
        let mut budgets = std::collections::HashMap::new();
        budgets.insert(
            tenant_id,
            budget::TenantBudget {
                limit_cents: 0,
                spent_cents: 0,
                cheap_fallback_model: None,
                guard_enabled: false,
                router_enabled: true,
                router_threshold: 0.35,
            },
        );
        let store: std::sync::Arc<dyn budget::BudgetStore> =
            std::sync::Arc::new(budget::InMemoryBudgetStore::new(budgets));
        let (response, mut rx) = traced_request_with_budget(
            &guarded_stream_body("gpt-4o-mini", "wrong-shape"),
            Some(CLIENT_KEY),
            store,
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let expected = delta_only_stream(r#"{"other":1}"#);
        assert_eq!(
            body.as_ref(),
            expected.as_bytes(),
            "opt-out tenant gets the raw cheap body, no fallback"
        );
        let msg = rx.recv().await.expect("trace emitted");
        assert_eq!(msg.request_model, "gpt-4o-mini");
        assert!(
            msg.guard_event.is_none(),
            "opt-out tenant records no guard event"
        );
    }

    #[tokio::test]
    async fn unguarded_stream_stays_byte_for_byte_unchanged() {
        // Same cheap model but NO schema contract: pure passthrough, even for
        // a body the guard would reject (IT-023 AC2: unguarded = zero change).
        let body = r#"{"model":"nodone","stream":true,"messages":[{"role":"user","content":"hi"}]}"#;
        let (response, _rx) = traced_request(body, Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let out = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(
            text.contains("data: {") && !text.contains("[DONE]"),
            "unguarded traffic pipes the truncated upstream body straight through"
        );
    }

    #[tokio::test]
    async fn frontier_model_with_schema_contract_is_not_guarded() {
        // Same schema contract but a frontier model the router does not
        // downgrade: passthrough, never guarded. (`deepseek-v4-pro` is the
        // wrong choice here — ITEM-054 downshifts it to a cheap tier.)
        let (response, mut rx) = traced_request(&guarded_stream_body("deepseek-reasoner", "hi"), Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let out = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(out.as_ref(), SSE_BODY.as_bytes());
        let msg = rx.recv().await.expect("trace emitted");
        // 026 AC4: guard telemetry is additive — unguarded traces stay None.
        assert!(msg.guard_event.is_none(), "unguarded trace never gains a guard event");
    }

    #[tokio::test]
    async fn a_router_downgraded_contract_request_is_guarded() {
        // ITEM-054 downshifts deepseek-v4-pro to deepseek-flash, which is a
        // cheap tier — so the guard engages on the downgraded request. This
        // silently did not happen while `is_cheap_tier` omitted deepseek-flash.
        let (response, mut rx) =
            traced_request(&guarded_stream_body("deepseek-v4-pro", "hi"), Some(CLIENT_KEY)).await;
        let _ = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let msg = rx.recv().await.expect("trace emitted");
        assert!(
            msg.guard_event.is_some(),
            "the router-downgraded cheap tier is guarded"
        );
    }

    /// Drive a real guarded request through axum::serve and drop the socket
    /// while the handler is still buffering (abort mid-buffer). `marker`
    /// selects the mock upstream behaviour for the cheap model: "guarded-slow"
    /// = slow cheap stream (abort before the guard verdict); "slow-fallback" =
    /// instant cheap violation then a slow frontier replay (abort mid-replay).
    async fn serve_guarded_and_abort(model: &str, marker: &str) -> mpsc::Receiver<TraceMessage> {
        let (tx, rx) = mpsc::channel(8);
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: mock_upstream().await,
            tenant_store: std::sync::Arc::new(seeded_store()),
            provider_store: std::sync::Arc::new(crate::providers::InMemoryProviderStore::default()),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            affinity_store: std::sync::Arc::new(crate::affinity::InMemoryAffinityStore::default()),
            policy_store: std::sync::Arc::new(crate::policy::InMemoryPolicyStore::default()),
            edge_store: std::sync::Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy(
                "postgres://inferon:inferon@localhost:5432/inferon",
            )
            .unwrap(),
        });
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, build_router(state)).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let body = guarded_stream_body(model, marker);
        let request = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAuthorization: Bearer {CLIENT_KEY}\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        socket.write_all(request.as_bytes()).await.unwrap();
        // The guard buffers everything, so the client sees no body bytes yet —
        // the abort happens while the handler still waits upstream (300ms gap
        // between frames on the mock). Drop mid-buffer.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        drop(socket); // abort
        rx
    }

    #[tokio::test]
    async fn client_abort_during_guarded_buffer_records_truncated_trace() {
        let mut rx = serve_guarded_and_abort("gpt-4o-mini", "guarded-slow").await;
        let msg = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
            .await
            .expect("truncated trace should be emitted")
            .expect("channel open");
        assert!(msg.truncated, "abort while buffering must be flagged truncated");
        // Exactly one trace — no duplicate from a later flush.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert!(rx.try_recv().is_err(), "no duplicate trace after abort");
    }

    #[tokio::test]
    async fn client_abort_during_frontier_replay_records_one_guard_trace() {
        // 025: cheap response violated instantly, so the replay to the frontier
        // (deepseek-v4-pro, slow) is in flight when the client aborts mid-
        // replay. Exactly one trace: truncated, retargeted to the frontier,
        // still carrying the guard event.
        let mut rx = serve_guarded_and_abort("gpt-4o-mini", "slow-fallback").await;
        let msg = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
            .await
            .expect("truncated trace should be emitted")
            .expect("channel open");
        assert!(msg.truncated, "abort during the replay is flagged truncated");
        assert_eq!(msg.request_model, "deepseek-v4-pro");
        let ev = msg.guard_event.as_ref().expect("guard event recorded");
        assert_eq!(ev["outcome"], "fallback");
        assert_eq!(ev["reason"], "schema_violation");
        assert_eq!(ev["guarded_model"], "gpt-4o-mini");
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert!(rx.try_recv().is_err(), "no duplicate trace after abort");
    }
}
