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

/// The shared store seam result: any `Send + Sync` error, so Postgres and
/// in-memory implementations can satisfy the same trait.
pub type StoreResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

// Control-plane surfaces.
pub mod auth;
pub mod dashboard;
pub mod edge;
pub mod edge_runtime;
// Data plane.
pub mod proxy;
pub mod promptreg;
// Routing intelligence.
pub mod affinity;
pub mod audit;
pub mod alerts;
pub mod budget;
pub mod capture;
pub mod cost;
pub mod datasets;
pub mod evolution;
pub mod guard;
pub mod heatmap;
pub mod hotpath;
pub mod linter;
pub mod loadbalance;
pub mod pii;
pub mod policy;
pub mod preflight;
pub mod prune;
pub mod quality;
pub mod ratelimit;
pub mod retention;
pub mod router;
pub mod routing;
pub mod signatures;
pub mod usage;
// Stores (trait seam + Postgres + in-memory).
pub mod models;
pub mod providers;
pub mod tenant;
pub mod telemetry;
pub mod translate;
// Shared primitives.
pub mod clock;
pub mod embed;
pub mod errors;
pub mod eval;
pub mod metrics;
pub mod secret;
pub mod session;
pub mod sse;
pub mod stages;
// Smoke-stack mocks (same binary).
pub mod mock_github;
pub mod mock_upstream;

pub use tenant::{ApiKeyInfo, InMemoryTenantStore, PostgresTenantStore, Tenant, TenantStore};
pub use telemetry::{load_system_prompt_counts, telemetry_worker, PostgresTraceSink, TraceMessage, TraceSink};
pub use usage::{usage_from_sse_bytes, Usage};

use crate::proxy::{dismiss_lint, healthz, list_models, proxy_chat_completions, proxy_messages, quality_signal};

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
    /// ITEM-092: per-tenant read cache for the request path (config, route,
    /// spend, warm target); authentication stays a fresh lookup.
    pub hotpath: Arc<hotpath::HotPath>,
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
        .route("/v1/lint/dismiss", post(dismiss_lint))
        .route("/v1/models", get(list_models))
        .route("/v1/dashboard/api/summary", get(dashboard::summary))
        .route("/v1/dashboard/api/performance", get(dashboard::performance))
        .route("/v1/dashboard/api/datasets", get(dashboard::datasets))
        .route("/v1/dashboard/api/sessions", get(dashboard::sessions))
        .route(
            "/v1/dashboard/api/datasets/purge",
            post(dashboard::purge),
        )
        .route(
            "/v1/dashboard/api/prompts",
            get(dashboard::prompts::prompts),
        )
        .route(
            "/v1/dashboard/api/prompts/labels",
            post(dashboard::prompts::label),
        )
        .route(
            "/v1/dashboard/api/prompts/labels/:name/:hash",
            axum::routing::delete(dashboard::prompts::unlabel),
        )
        .route(
            "/v1/dashboard/api/prompts/diff",
            get(dashboard::prompts::diff),
        )
        .route("/v1/dashboard/api/evals", get(dashboard::evals::list))
        .route(
            "/v1/dashboard/api/evals/gold",
            post(dashboard::evals::pin_gold),
        )
        .route(
            "/v1/dashboard/api/evals/run",
            post(dashboard::evals::start_run),
        )
        .route(
            "/v1/dashboard/api/evals/compare",
            post(dashboard::evals::compare),
        )
        .route(
            "/v1/dashboard/api/datasets/export",
            get(dashboard::export),
        )
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
        .route(
            "/v1/dashboard/api/routing/tier-pairs",
            post(dashboard::tenant_tier_pairs_set),
        )
        .route("/v1/dashboard/api/config", get(dashboard::tenant_config).post(dashboard::tenant_config_set))
        .route("/v1/dashboard/api/by-model", get(dashboard::by_model))
        .route("/v1/dashboard/api/by-prompt", get(dashboard::by_prompt))
        .route("/v1/dashboard/api/by-provider", get(dashboard::by_provider))
        .route("/v1/dashboard/api/by-key", get(dashboard::by_key))
        .route("/v1/public/audit/:token", get(audit::public_report))
        .route(
            "/v1/dashboard/api/audit/shares",
            get(dashboard::tenant_audit_shares).post(dashboard::tenant_audit_share_create),
        )
        .route(
            "/v1/dashboard/api/audit/shares/:id/revoke",
            post(dashboard::tenant_audit_share_revoke),
        )
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
            "/v1/dashboard/api/operator/evolution",
            get(dashboard::operator_evolution),
        )
        .route(
            "/v1/dashboard/api/operator/evolution/detect",
            post(dashboard::operator_evolution_detect),
        )
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
    state: &Arc<AppState>,
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
    // ITEM-092: cached for 1 s and invalidated on every key write, so the warm
    // path is CPU-only while revocation stays immediate in this process.
    state.hotpath.tenant_for_key(state, &sha256_hex(key)).await
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
mod tests;
