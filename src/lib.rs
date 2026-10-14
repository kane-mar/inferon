// Every request-path helper returns its early-return HTTP response directly as
// the error (`Result<T, Response>`) — the axum idiom. Boxing the response at
// ~10 signatures would only add noise.
#![allow(clippy::result_large_err)]

pub mod alerts;
pub mod affinity;
pub mod budget;
pub mod guard;
use guard::GuardReason;
pub mod mock_github;
pub mod mock_upstream;
pub mod cost;
pub mod auth;
pub mod dashboard;
pub mod edge;
pub mod embed;
pub mod errors;
pub mod linter;
pub mod metrics;
pub mod models;
pub mod policy;
pub mod providers;
pub mod quality;
pub mod router;
pub mod routing;
pub mod secret;
pub mod session;
pub mod sse;
pub mod tenant;
pub mod telemetry;
pub mod usage;

pub use tenant::{ApiKeyInfo, InMemoryTenantStore, PostgresTenantStore, Tenant, TenantStore};
pub use telemetry::{load_system_prompt_counts, telemetry_worker, PostgresTraceSink, TraceMessage, TraceSink};
pub use usage::{usage_from_sse_bytes, Usage};

use crate::usage::{
    system_prompt_hash, system_prompt_hash_anthropic, usage_from_anthropic_json,
    usage_from_anthropic_sse_bytes, usage_from_json,
};
use crate::quality::{outcome_for_json, outcome_for_sse, Outcome};
use crate::errors::ErrorClass;

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header::{AUTHORIZATION, CONTENT_TYPE}, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::{Stream, StreamExt};
use reqwest::Client;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    sync::Arc,
    time::Instant,
};
use tokio::sync::mpsc;
use uuid::Uuid;

const MAX_REQUEST_BYTES: usize = 10 * 1024 * 1024;

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

async fn healthz() -> &'static str {
    "ok"
}

/// GET /v1/models — the tenant's available models in the OpenAI list shape, so
/// SDKs can discover them. It's the union of the providers' advertised models
/// (cached), minus disabled ones; a tenant with nothing cached yet discovers
/// its default provider's list once.
async fn list_models(State(state): State<Arc<AppState>>, req: Request) -> Response {
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
    let providers = match state.provider_store.list(tenant.id).await {
        Ok(providers) => providers,
        Err(e) => {
            tracing::error!("provider list failed: {e}");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "provider_error",
                "could not resolve your providers",
            );
        }
    };
    let mut ids: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for provider in &providers {
        let advertised = state
            .provider_store
            .advertised_models(provider.id)
            .await
            .unwrap_or_default();
        let settings = state
            .model_store
            .settings(provider.id)
            .await
            .unwrap_or_default();
        for id in advertised {
            if !settings
                .iter()
                .any(|setting| setting.model_id == id && !setting.enabled)
            {
                ids.insert(id);
            }
        }
    }
    // A freshly added provider has nothing cached yet — discover once. With no
    // provider at all, the shared platform key serves traffic, so list its
    // host's models (the global upstream).
    if ids.is_empty() {
        if let Ok(Some(provider)) = state.provider_store.resolve_for(tenant.id, None).await {
            match dashboard::fetch_provider_models(&state, &provider).await {
                Ok(fetched) => {
                    let _ = state.provider_store.record_models(provider.id, &fetched).await;
                    ids.extend(fetched);
                }
                Err(e) => tracing::warn!("model discovery failed for provider {}: {e}", provider.id),
            }
        } else if let Some(key) = tenant.upstream_openai_key.as_deref() {
            let shared = crate::providers::ResolvedProvider {
                id: Uuid::nil(),
                kind: "openai".to_string(),
                base_url: dashboard::upstream_base_url(&state.upstream_url),
                api_key: key.to_string(),
                metadata: crate::providers::ProviderMetadata::default(),
            };
            match dashboard::fetch_provider_models(&state, &shared).await {
                Ok(fetched) => ids.extend(fetched),
                Err(e) => tracing::warn!("model discovery failed for the shared upstream: {e}"),
            }
        }
    }
    let data: Vec<Value> = ids
        .into_iter()
        .map(|id| json!({ "id": id, "object": "model", "created": 0, "owned_by": "inferon" }))
        .collect();
    Json(json!({ "object": "list", "data": data })).into_response()
}

/// The resolved upstream target for one request (ITEM-043): the tenant's own
/// provider that owns the requested model, else its first provider, else the
/// shared key + the global URL.
#[derive(Clone)]
struct Route {
    url: String,
    key: String,
    /// Extra upstream headers from the tenant's provider (ITEM-043 AC1).
    metadata: providers::ProviderMetadata,
    /// ITEM-047: the provider row that served the request (`None` = shared key).
    provider_id: Option<Uuid>,
    /// ITEM-050: the provider kind, which decides the upstream auth + surface.
    kind: String,
}

/// The ingress→upstream surface (ITEM-050): OpenAI is the chat-completions
/// shape, Anthropic the Messages shape. A request never crosses surfaces.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Surface {
    OpenAi,
    Anthropic,
}

impl Surface {
    /// `POST <base>/chat/completions` or `POST <base>/messages`.
    fn endpoint(self, base_url: &str) -> String {
        let base = base_url.trim_end_matches('/');
        match self {
            Surface::OpenAi => format!("{base}/chat/completions"),
            Surface::Anthropic => format!("{base}/messages"),
        }
    }
}

/// Attach the tenant's optional provider headers (org + custom) to an upstream
/// request. An empty metadata is a no-op.
fn apply_provider_headers(
    mut request: reqwest::RequestBuilder,
    metadata: &providers::ProviderMetadata,
) -> reqwest::RequestBuilder {
    if let Some(org) = &metadata.org {
        request = request.header("OpenAI-Organization", org);
    }
    for (header, value) in &metadata.headers {
        request = request.header(header, value);
    }
    request
}

async fn resolve_route(
    state: &Arc<AppState>,
    tenant: &Tenant,
    model: Option<&str>,
    surface: Surface,
) -> Result<Route, Response> {
    match state.provider_store.resolve_for(tenant.id, model).await {
        Ok(Some(provider)) => {
            // ITEM-050 AC5: never cross surfaces — an Anthropic provider serves
            // only /v1/messages, an OpenAI-shaped one only /v1/chat/completions.
            if providers::is_anthropic(&provider.kind) != (surface == Surface::Anthropic) {
                return Err(surface_mismatch(surface));
            }
            Ok(Route {
                url: surface.endpoint(&provider.base_url),
                key: provider.api_key,
                metadata: provider.metadata,
                provider_id: Some(provider.id),
                kind: provider.kind,
            })
        }
        Ok(None) => {
            if surface == Surface::Anthropic {
                return Err(surface_mismatch(surface));
            }
            match tenant.upstream_openai_key.as_deref() {
                Some(key) => Ok(Route {
                    url: state.upstream_url.clone(),
                    key: key.to_string(),
                    metadata: providers::ProviderMetadata::default(),
                    provider_id: None,
                    kind: "openai".to_string(),
                }),
                None => Err(json_error(
                    StatusCode::BAD_REQUEST,
                    "no_provider",
                    "no upstream provider configured — add one in Dashboard → Providers",
                )),
            }
        }
        Err(e) => {
            tracing::error!("provider resolution failed: {e}");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "provider_error",
                "could not resolve your provider",
            ))
        }
    }
}

/// A clear typed error when the resolved provider's kind does not match the
/// surface the request arrived on (ITEM-050 AC5).
fn surface_mismatch(surface: Surface) -> Response {
    let message = match surface {
        Surface::Anthropic => {
            "no Anthropic provider configured — add one in Dashboard → Providers"
        }
        Surface::OpenAi => {
            "no OpenAI-compatible provider configured — add one in Dashboard → Providers"
        }
    };
    json_error(StatusCode::BAD_REQUEST, "no_provider", message)
}

/// Read and parse a JSON request body, or the typed error response.
/// Returns the raw bytes too — the Anthropic surface forwards them unchanged.
async fn read_json_body(req: Request) -> Result<(axum::body::Bytes, Value), Response> {
    let body_bytes = axum::body::to_bytes(req.into_body(), MAX_REQUEST_BYTES)
        .await
        .map_err(|_| {
            json_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "request body exceeds 10MB",
            )
        })?;
    let payload = serde_json::from_slice(&body_bytes).map_err(|_| {
        json_error(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            "request body must be valid JSON",
        )
    })?;
    Ok((body_bytes, payload))
}

/// The ordered, de-duplicated `(model, route)` chain to attempt: the primary
/// (after any router/budget override), then the tenant's fallbacks, dropping
/// models the tenant disabled (ITEM-044). Each entry carries the provider that
/// owns it, so a fallback to another provider's model hits the right upstream.
/// Always contains at least the primary.
async fn build_model_chain(
    state: &Arc<AppState>,
    tenant: &Tenant,
    route: &Route,
    primary: String,
    fallbacks: Vec<String>,
) -> Vec<(String, Route)> {
    let mut seen = std::collections::HashSet::new();
    let mut chain = Vec::new();
    for model in std::iter::once(primary.clone()).chain(fallbacks) {
        if !seen.insert(model.clone()) {
            continue;
        }
        let model_route = if model == primary {
            route.clone()
        } else {
            resolve_route(state, tenant, Some(&model), Surface::OpenAi)
                .await
                .unwrap_or_else(|_| route.clone())
        };
        if state
            .model_store
            .is_disabled(model_route.provider_id, &model)
            .await
            .unwrap_or(false)
        {
            continue;
        }
        chain.push((model, model_route));
    }
    if chain.is_empty() {
        chain.push((primary, route.clone()));
    }
    chain
}

/// Proxy a chat completion request to the configured upstream.
///
/// Authenticates the caller via `Authorization: Bearer <key>` (SHA-256 lookup
/// against the tenant store), swaps the upstream auth header to the tenant's
/// master key, then forwards: streaming requests pipe upstream bytes through
/// unchanged (zero-copy) as text/event-stream; non-streaming requests pass the
/// JSON body through with usage intact. Transport errors become 502; upstream
/// HTTP error statuses are forwarded as-is.
async fn proxy_chat_completions(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Response {
    let _in_flight = crate::metrics::request_started();
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

    let (_, payload) = match read_json_body(req).await {
        Ok(parsed) => parsed,
        Err(resp) => return resp,
    };
    let is_stream = payload["stream"].as_bool().unwrap_or(false);
    // The router/budget override is decided per request (ITEM-045/046) — see
    // `check_budget` below, which applies both against the tenant's config.
    let mut meta = RequestMeta {
        id: Uuid::new_v4(),
        model: payload["model"].as_str().unwrap_or("unknown").to_string(),
        system_prompt_hash: system_prompt_hash(&payload),
        raw_prompt: payload,
        routing_event: None,
        router_event: None,
        cache_affinity: None,
        policy_version: None,
        provider_id: None,
        api_key_id: tenant.api_key_id,
    };
    let route = match resolve_route(&state, &tenant, Some(&meta.model), Surface::OpenAi).await {
        Ok(route) => route,
        Err(resp) => return resp,
    };
    meta.provider_id = route.provider_id;
    crate::metrics::request_routed(route.provider_id);

    let started = Instant::now();

    // ITEM-044 AC5: a model the tenant disabled is refused with a clear typed
    // error (and traced), not a raw upstream 4xx.
    if let Err(resp) =
        ensure_model_enabled(&state, &meta, tenant_id, started, route.provider_id, &meta.model)
            .await
    {
        return resp;
    }

    // ITEM-064: the adaptive policy may override the router threshold.
    let policy = match state.policy_store.active(tenant_id).await {
        Ok(policy) => policy.filter(|policy| policy.enabled),
        Err(e) => {
            tracing::warn!("policy lookup failed: {e}");
            None
        }
    };
    meta.policy_version = policy.as_ref().map(|policy| policy.version);
    let policy_threshold = policy.as_ref().and_then(|policy| policy.router_threshold);
    // Budget breaker (ITEM-016): its fallback model wins over the router;
    // a hard-cap rejection is traced and answered 429 before any upstream call.
    let budget_override = match budget::check_budget(
        &state,
        tenant_id,
        &meta.raw_prompt,
        policy_threshold,
    )
    .await
    {
        Ok(model) => model,
        Err(resp) => {
            trace_error(&state, &meta, tenant_id, started, 429, ErrorClass::Inferon4xx);
            return resp;
        }
    };
    let mut model_override = budget_override;
    // ITEM-044 AC4: routing never selects a model the tenant disabled — drop the
    // override and serve the (already-allowed) requested model. ITEM-054: when
    // the heuristic router caused the override, record it on the trace.
    if let Some(chosen) = model_override.clone() {
        if state
            .model_store
            .is_disabled(route.provider_id, &chosen.model)
            .await
            .unwrap_or(false)
        {
            model_override = None;
        } else if chosen.reason == budget::ROUTER_REASON {
            meta.router_event = Some(json!({
                "from_model": meta.model,
                "to_model": chosen.model,
                "reason": chosen.reason,
            }));
        }
    }
    // ITEM-045: the ordered chain — the primary (after any router/budget override),
    // then the tenant's fallbacks, de-duped and with disabled models dropped
    // (ITEM-044). Loaded once per request; an empty chain changes nothing.
    let primary = model_override.take().map(|chosen| chosen.model).unwrap_or_else(|| meta.model.clone());
    // ITEM-063: prefer a warm provider cache over a marginally cheaper miss.
    let primary = match meta.system_prompt_hash.as_deref() {
        Some(hash) => match state.affinity_store.warm_target(tenant_id, hash).await {
            Ok(Some(warm)) if warm.model != primary && crate::affinity::keep_warm(&warm.model, &primary) => {
                meta.cache_affinity = Some(json!({
                    "from_model": primary,
                    "to_model": warm.model,
                    "reason": "cache_affinity",
                }));
                warm.model
            }
            _ => primary,
        },
        None => primary,
    };
    let fallbacks = match state.routing_store.fallbacks(tenant_id).await {
        Ok(models) => models,
        Err(e) => {
            tracing::error!("routing policy load failed: {e}");
            Vec::new()
        }
    };
    let allowed = build_model_chain(&state, &tenant, &route, primary.clone(), fallbacks).await;

    let mut routing_event: Option<Value> = None;
    let mut upstream: Option<reqwest::Response> = None;
    for index in 0..allowed.len() {
        let (model, attempt_route) = (&allowed[index].0, &allowed[index].1);
        let model = model.clone();
        let body = match budget::apply_model_override(&meta.raw_prompt, &model) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!("payload reserialization failed: {e}");
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "serialization_error",
                    "failed to reserialize the request payload",
                );
            }
        };
        let attempt = apply_provider_headers(
            state
                .http_client
                .post(&attempt_route.url)
                .header("Authorization", format!("Bearer {}", attempt_route.key))
                .header("Content-Type", "application/json"),
            &attempt_route.metadata,
        )
        .body(body)
        .send()
        .await;
        let is_last = index + 1 == allowed.len();
        match attempt {
            Ok(response) => {
                let status = response.status();
                let reason = routing::fallback_reason(status.as_u16());
                if status.is_success() || is_last || reason.is_none() {
                    meta.model = model;
                    upstream = Some(response);
                    break;
                }
                let next = allowed[index + 1].0.clone();
                tracing::warn!(
                    "routing fallback: {model} -> {next} (HTTP {})",
                    status.as_u16()
                );
                routing_event = Some(json!({
                    "from_model": primary,
                    "to_model": next,
                    "reason": reason.expect("retryable").label(),
                }));
            }
            Err(e) => {
                if is_last {
                    tracing::error!("upstream request failed: {e}");
                    trace_error(
                        &state,
                        &meta,
                        tenant_id,
                        started,
                        502,
                        ErrorClass::from_transport(&e),
                    );
                    return json_error(
                        StatusCode::BAD_GATEWAY,
                        "upstream_error",
                        "upstream request failed",
                    );
                }
                let next = allowed[index + 1].0.clone();
                tracing::warn!("routing fallback (transport): {model} -> {next}");
                routing_event = Some(json!({
                    "from_model": primary,
                    "to_model": next,
                    "reason": routing::FallbackReason::Transport.label(),
                }));
            }
        }
    }
    meta.routing_event = routing_event;
    let upstream = upstream.expect("the chain always has at least one model");

    let status = upstream.status();
    if !status.is_success() {
        // Forward the upstream error as-is (keep its content-type); still trace.
        return forward_buffered(&state, &meta, tenant_id, started, upstream, None, usage_from_json).await;
    }
    if is_stream {
        // ITEM-023 (guard): requests with a JSON-schema contract served by a
        // cheap tier are buffered end-to-end and validated before anything
        // reaches the client. Anything else keeps the zero-copy passthrough.
        // ITEM-027 AC1: the guard is ON by default; a tenant opts out via
        // tenant_configurations.guard_enabled (fail-open on read errors).
        let guarded = guard::is_guarded_request(&meta.raw_prompt, &meta.model)
            && state
                .budget_store
                .guard_enabled_for(tenant_id)
                .await
                .unwrap_or(true);
        if guarded {
            guarded_stream_response(
                &state,
                meta,
                tenant_id,
                started,
                upstream,
                &route,
                state.telemetry_tx.clone(),
            )
            .await
        } else {
            stream_response(
                upstream,
                started,
                meta,
                tenant_id,
                status.as_u16(),
                state.telemetry_tx.clone(),
                usage_from_sse_bytes,
            )
        }
    } else {
        let json_content_type = Some(HeaderValue::from_static("application/json"));
        forward_buffered(&state, &meta, tenant_id, started, upstream, json_content_type, usage_from_json).await
    }
}

/// Proxy an Anthropic Messages request (ITEM-050).
///
/// Authenticates the caller with `x-api-key` (the Anthropic SDK's header; a
/// Bearer token is tolerated), resolves an `anthropic` provider, and forwards
/// the Anthropic-shaped body to `<base>/messages` with the provider's key. The
/// heuristic router and schema guard are OpenAI-shaped and explicitly bypassed;
/// the hard budget cap still applies. Usage (`input_tokens`/`output_tokens`) is
/// traced and costed.
async fn proxy_messages(State(state): State<Arc<AppState>>, req: Request) -> Response {
    let _in_flight = crate::metrics::request_started();
    let tenant = match authenticate_anthropic(&state, req.headers()).await {
        Ok(Some(tenant)) => tenant,
        Ok(None) => {
            return json_error(
                StatusCode::UNAUTHORIZED,
                "invalid_api_key",
                "missing or invalid x-api-key header",
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

    let (body_bytes, payload) = match read_json_body(req).await {
        Ok(parsed) => parsed,
        Err(resp) => return resp,
    };
    let is_stream = payload["stream"].as_bool().unwrap_or(false);
    let mut meta = RequestMeta {
        id: Uuid::new_v4(),
        model: payload["model"].as_str().unwrap_or("unknown").to_string(),
        system_prompt_hash: system_prompt_hash_anthropic(&payload),
        raw_prompt: payload,
        routing_event: None,
        router_event: None,
        cache_affinity: None,
        policy_version: None,
        provider_id: None,
        api_key_id: tenant.api_key_id,
    };
    let route = match resolve_route(&state, &tenant, Some(&meta.model), Surface::Anthropic).await {
        Ok(route) => route,
        Err(resp) => return resp,
    };    meta.provider_id = route.provider_id;
    crate::metrics::request_routed(route.provider_id);
    let started = Instant::now();

    if let Err(resp) =
        ensure_model_enabled(&state, &meta, tenant_id, started, route.provider_id, &meta.model)
            .await
    {
        return resp;
    }

    // ITEM-050 AC5: the heuristic router and cheap-fallback are OpenAI-shaped
    // and bypassed here; only the hard monthly cap still applies.
    if let Err(resp) = budget::check_cap(&state, tenant_id).await {
        trace_error(&state, &meta, tenant_id, started, 429, ErrorClass::Inferon4xx);
        return resp;
    }

    let mut request = state
        .http_client
        .post(&route.url)
        .header("Content-Type", "application/json");
    for (header, value) in providers::auth_headers(&route.kind, &route.key, &route.metadata) {
        request = request.header(header, value);
    }
    let request = apply_provider_headers(request, &route.metadata);
    let upstream = match request.body(body_bytes).send().await {
        Ok(response) => response,
        Err(e) => {
            tracing::error!("anthropic upstream request failed: {e}");
            trace_error(
                &state,
                &meta,
                tenant_id,
                started,
                502,
                ErrorClass::from_transport(&e),
            );
            return json_error(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                "upstream request failed",
            );
        }
    };

    let status = upstream.status();
    if !status.is_success() {
        return forward_buffered(
            &state,
            &meta,
            tenant_id,
            started,
            upstream,
            None,
            usage_from_anthropic_json,
        )
        .await;
    }
    if is_stream {
        stream_response(
            upstream,
            started,
            meta,
            tenant_id,
            status.as_u16(),
            state.telemetry_tx.clone(),
            usage_from_anthropic_sse_bytes,
        )
    } else {
        let json_content_type = Some(HeaderValue::from_static("application/json"));
        forward_buffered(
            &state,
            &meta,
            tenant_id,
            started,
            upstream,
            json_content_type,
            usage_from_anthropic_json,
        )
        .await
    }
}

/// Resolve the caller's tenant from `x-api-key` (Anthropic), tolerating a
/// Bearer token for OpenAI-compatible Anthropic gateways.
async fn authenticate_anthropic(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>> {
    let key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .filter(|key| !key.is_empty());
    match key {
        Some(key) => state.tenant_store.tenant_by_api_key_hash(&sha256_hex(key)).await,
        None => authenticate(state, headers).await,
    }
}

/// Buffer an upstream body, trace the completed request, and build the
/// forwarded response. `content_type` overrides the forwarded content-type
/// when present; otherwise the upstream's own header is preserved (upstream
/// error bodies are forwarded as-is). Shared by the non-2xx and the 2xx
/// non-streaming paths.
async fn forward_buffered(
    state: &AppState,
    meta: &RequestMeta,
    tenant_id: Uuid,
    started: Instant,
    upstream: reqwest::Response,
    content_type: Option<HeaderValue>,
    parse_usage: fn(&Option<Value>) -> Usage,
) -> Response {
    let status = upstream.status();
    let upstream_content_type = upstream.headers().get(CONTENT_TYPE).cloned();
    let bytes = match upstream.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("upstream body read failed: {e}");
            return json_error(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                "upstream request failed",
            );
        }
    };
    let response_json: Option<Value> = serde_json::from_slice(&bytes).ok();
    let usage = parse_usage(&response_json);
    let outcome = outcome_for_json(status.as_u16(), &response_json);
    let error_class = (!status.is_success())
        .then(|| ErrorClass::from_upstream_status(status.as_u16()));
    let total_ms = started.elapsed().as_millis() as i64;
    // Non-streaming ttft mirrors total duration; error responses have no ttft.
    let ttft_ms = status.is_success().then_some(total_ms);
    let msg = build_trace(
        meta,
        tenant_id,
        TraceOutcome {
            status: status.as_u16(),
            total_duration_ms: total_ms,
            usage,
            ttft_ms,
            raw_completion: response_json,
            truncated: false,
            outcome,
            error_class,
        },
    );
    send_trace(&state.telemetry_tx, msg);

    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = status;
    if let Some(content_type) = content_type.or(upstream_content_type) {
        response.headers_mut().insert(CONTENT_TYPE, content_type);
    }
    with_trace_id(response, meta.id)
}

/// Request-derived fields shared by every trace of this request.
#[derive(Clone)]
struct RequestMeta {
    id: Uuid,
    model: String,
    system_prompt_hash: Option<String>,
    raw_prompt: Value,
    /// ITEM-045: set when a fallback chain changed the serving model.
    routing_event: Option<Value>,
    /// ITEM-054: set when the heuristic router downshifted the model.
    router_event: Option<Value>,
    /// ITEM-063: set when cache affinity changed the serving model.
    cache_affinity: Option<Value>,
    /// ITEM-064: the adaptive policy version in force for this request.
    policy_version: Option<i64>,
    /// ITEM-047: provider + API key attribution for the trace.
    provider_id: Option<Uuid>,
    api_key_id: Option<Uuid>,
}

/// The outcome of a proxied request, as recorded on its trace.
struct TraceOutcome {
    status: u16,
    total_duration_ms: i64,
    usage: Usage,
    ttft_ms: Option<i64>,
    raw_completion: Option<Value>,
    truncated: bool,
    /// ITEM-055: whether the response was usable.
    outcome: Outcome,
    /// ITEM-057: why a failure/abort happened (`None` on success).
    error_class: Option<ErrorClass>,
}

impl TraceOutcome {
    /// A terminal error response with no usage or completion body.
    fn error(status: u16, total_duration_ms: i64, error_class: ErrorClass) -> Self {
        Self {
            status,
            total_duration_ms,
            usage: Usage::default(),
            ttft_ms: None,
            raw_completion: None,
            truncated: false,
            outcome: Outcome::Error,
            error_class: Some(error_class),
        }
    }
}

fn build_trace(meta: &RequestMeta, tenant_id: Uuid, outcome: TraceOutcome) -> TraceMessage {
    TraceMessage {
        id: meta.id,
        tenant_id,
        request_model: meta.model.clone(),
        system_prompt_hash: meta.system_prompt_hash.clone(),
        prompt_tokens: outcome.usage.prompt_tokens,
        completion_tokens: outcome.usage.completion_tokens,
        cached_prompt_tokens: outcome.usage.cached_tokens,
        ttft_ms: outcome.ttft_ms,
        total_duration_ms: outcome.total_duration_ms,
        status_code: outcome.status,
        raw_prompt: Some(meta.raw_prompt.clone()),
        raw_completion: outcome.raw_completion,
        waste_detected: None,
        upstream_cost_usd: rust_decimal::Decimal::ZERO,
        cache_savings_usd: rust_decimal::Decimal::ZERO,
        truncated: outcome.truncated,
        guard_event: None,
        routing_event: meta.routing_event.clone(),
        router_event: meta.router_event.clone(),
        provider_id: meta.provider_id,
        api_key_id: meta.api_key_id,
        outcome: outcome.outcome,
        error_class: outcome.error_class,
        savings_usd: rust_decimal::Decimal::ZERO,
        savings_kind: None,
        cache_affinity: meta.cache_affinity.clone(),
        policy_version: meta.policy_version,
    }
}

/// Refuse a model the tenant disabled with a typed, traced 400 (ITEM-044 AC5).
async fn ensure_model_enabled(
    state: &Arc<AppState>,
    meta: &RequestMeta,
    tenant_id: Uuid,
    started: Instant,
    provider_id: Option<Uuid>,
    model: &str,
) -> Result<(), Response> {
    match state.model_store.is_disabled(provider_id, model).await {
        Ok(false) => Ok(()),
        Ok(true) => {
            trace_error(state, meta, tenant_id, started, 400, ErrorClass::Inferon4xx);
            Err(json_error(
                StatusCode::BAD_REQUEST,
                "model_disabled",
                "this model is disabled for your tenant",
            ))
        }
        Err(e) => {
            tracing::error!("model selection lookup failed: {e}");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "model_store_error",
                "could not check model selection",
            ))
        }
    }
}

/// Trace a terminal error response (no usage, no completion body).
fn trace_error(
    state: &AppState,
    meta: &RequestMeta,
    tenant_id: Uuid,
    started: Instant,
    status: u16,
    error_class: ErrorClass,
) {
    let total_ms = started.elapsed().as_millis() as i64;
    let msg = build_trace(meta, tenant_id, TraceOutcome::error(status, total_ms, error_class));
    send_trace(&state.telemetry_tx, msg);
}

/// Queue a trace without ever blocking the request path (AC5): a full channel
/// drops the trace with an error log; a closed channel logs at debug level.
fn send_trace(tx: &mpsc::Sender<TraceMessage>, msg: TraceMessage) {
    match tx.try_send(msg) {
        Ok(()) => crate::metrics::count_queued(),
        Err(mpsc::error::TrySendError::Full(_)) => {
            crate::metrics::count_dropped(true);
            tracing::error!("telemetry channel full; trace dropped");
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            crate::metrics::count_dropped(false);
            tracing::debug!("telemetry channel closed; trace dropped");
        }
    }
}

/// Body of `POST /v1/quality` (ITEM-055).
#[derive(serde::Deserialize)]
struct QualitySignalBody {
    trace_id: Uuid,
    signal: String,
    #[serde(default)]
    score: Option<f64>,
}

/// POST /v1/quality — attach a client quality signal to a traced request.
///
/// Tenant-authenticated with the tenant's API key. The trace may not be
/// persisted yet (telemetry is async), so the signal is upserted into its own
/// table keyed by trace id; the dashboard joins the two. `X-Inferon-Trace-Id`
/// on a completion response gives the client the id to send here.
async fn quality_signal(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<QualitySignalBody>,
) -> Response {
    let tenant = match authenticate(&state, &headers).await {
        Ok(Some(tenant)) => tenant,
        Ok(None) => {
            return json_error(
                StatusCode::UNAUTHORIZED,
                "invalid_api_key",
                "missing or invalid Authorization header",
            )
        }
        Err(e) => {
            tracing::error!("quality signal tenant lookup failed: {e}");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "tenant_store_error",
                "tenant lookup failed",
            );
        }
    };
    if !matches!(body.signal.as_str(), "up" | "down") {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_signal",
            "signal must be \"up\" or \"down\"",
        );
    }
    let score = body.score.map(|score| score.clamp(0.0, 1.0));
    let result = sqlx::query(
        "INSERT INTO trace_quality (trace_id, tenant_id, signal, score)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (trace_id) DO UPDATE
           SET signal = EXCLUDED.signal, score = EXCLUDED.score, created_at = NOW()",
    )
    .bind(body.trace_id)
    .bind(tenant.id)
    .bind(&body.signal)
    .bind(score)
    .execute(&state.db)
    .await;
    match result {
        Ok(_) => (
            StatusCode::ACCEPTED,
            Json(json!({ "ok": true, "trace_id": body.trace_id.to_string() })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("quality signal insert failed: {e}");
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "quality_store_error",
                "could not store the quality signal",
            )
        }
    }
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

/// ITEM-055: tag a response with the request's trace id so a client can attach
/// a quality signal to it out of band.
fn with_trace_id(mut response: Response, trace_id: Uuid) -> Response {
    if let Ok(value) = HeaderValue::from_str(&trace_id.to_string()) {
        response.headers_mut().insert("x-inferon-trace-id", value);
    }
    response
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

/// Pipe an upstream byte stream through as text/event-stream, emitting the
/// request trace once the stream is fully consumed. Chunks are yielded
/// immediately (zero-copy; no buffering that would break TTFT) while bytes are
/// accumulated on the side solely for the final usage-frame parse (AC4).
fn stream_response(
    upstream: reqwest::Response,
    started: Instant,
    meta: RequestMeta,
    tenant_id: Uuid,
    status: u16,
    tx: mpsc::Sender<TraceMessage>,
    parse_usage: fn(&[u8]) -> Option<Usage>,
) -> Response {
    let byte_stream = upstream.bytes_stream();
    let trace_id = meta.id;
    let stream = stream_with_trace(byte_stream, started, meta, tenant_id, status, tx, parse_usage);
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    with_trace_id(response, trace_id)
}

/// Guarded buffered response (ITEM-023). For requests with a JSON-schema
/// contract served by a cheap tier the whole upstream body is buffered and
/// only released to the client after `[DONE]` + successful validation. A
/// client abort during the buffered phase still emits the truncated trace
/// (ITEM-009 semantics — Drop hook on `BufferedGuard`).
///
/// ITEM-025: when the flush gate rejects the cheap response (any classified
/// failure), the bad buffer is discarded and the original prompt is replayed
/// to the tenant's frontier model with their upstream key; the replacement
/// response is served to the client and the request's single trace records
/// the guard event. If the replay itself fails the request answers loudly
/// with a clean 502 (AC4) — never a silent drop.
async fn guarded_stream_response(
    state: &Arc<AppState>,
    meta: RequestMeta,
    tenant_id: Uuid,
    started: Instant,
    upstream: reqwest::Response,
    route: &Route,
    tx: mpsc::Sender<TraceMessage>,
) -> Response {
    let mut guard_emit = BufferedGuard::new(tx, meta, tenant_id, started);
    let bytes = match upstream.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("guarded upstream body read failed: {e}");
            // 025: a cheap stream that dies before any bytes is the
            // "connection crash" class — fall back, don't surface it.
            return fallback_to_frontier(&mut guard_emit, state, route, GuardReason::UpstreamFailure)
                .await;
        }
    };

    // Flush gate: container integrity then schema conformance (ITEM-024).
    let schema = guard::request_schema(&guard_emit.meta.raw_prompt);
    match guard::check_guarded_sse(&bytes, schema.as_ref()) {
        Ok(()) => {
            let usage = usage_from_sse_bytes(&bytes).unwrap_or_default();
            // ttft is when the client actually sees the first byte: after the
            // buffered flush (Slice 4 latency target measures this, ITEM-027).
            guard_emit.note_guard(None, guard::guard_flushed_event(&guard_emit.meta.model));
            guard_emit.emit(usage, false, outcome_for_sse(200, &bytes), None);
            let mut response = Response::new(Body::from(bytes));
            *response.status_mut() = StatusCode::OK;
            response
                .headers_mut()
                .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
            with_trace_id(response, guard_emit.meta.id)
        }
        Err(reason) => {
            tracing::warn!(
                "guard rejected a cheap-tier response: {} (reason {}) — replaying on the frontier",
                tenant_id,
                reason.label()
            );
            fallback_to_frontier(&mut guard_emit, state, route, reason).await
        }
    }
}

/// ITEM-025: transparent fallback after the guard discards a cheap response.
/// Replays the ORIGINAL prompt (`meta.raw_prompt`, pre-router/budget
/// downgrade) on the tenant's frontier model — their own key, same upstream —
/// and serves the replacement response. Exactly one trace is emitted for the
/// request (BufferedGuard is one-shot): on success it records the fallback
/// guard event and the trace's model is retargeted to the model that served
/// (the cost basis); when the replay itself fails the request answers a loud
/// clean 502 and the trace records `fallback_failed` + the reason (AC3/AC4).
async fn fallback_to_frontier(
    guard_emit: &mut BufferedGuard,
    state: &AppState,
    route: &Route,
    reason: GuardReason,
) -> Response {
    let frontier = guard::fallback_model(&guard_emit.meta.raw_prompt);
    let replay_body = match budget::apply_model_override(&guard_emit.meta.raw_prompt, &frontier) {
        Ok(body) => body,
        Err(e) => {
            tracing::error!("guard fallback payload reserialization failed: {e}");
            guard_emit.emit(Usage::default(), false, Outcome::Error, Some(ErrorClass::Inferon5xx));
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "serialization_error",
                "failed to reserialize the request payload",
            );
        }
    };
    let replay = apply_provider_headers(
        state
            .http_client
            .post(&route.url)
            .header("Authorization", format!("Bearer {}", route.key))
            .header("Content-Type", "application/json"),
        &route.metadata,
    )
    .body(replay_body)
    .send()
    .await;
    let guarded_model = guard_emit.meta.model.clone();
    match replay {
        Err(e) => {
            tracing::error!("guard frontier replay failed: {e}");
            let event =
                guard::guard_event("fallback_failed", reason, &guarded_model, Some("transport error"));
            guard_emit.note_guard(Some(&frontier), event);
            guard_emit.emit(
                Usage::default(),
                false,
                Outcome::Error,
                Some(ErrorClass::from_transport(&e)),
            );
            json_error(
                StatusCode::BAD_GATEWAY,
                "guard_fallback_error",
                "frontier fallback request failed",
            )
        }
        Ok(res) if !res.status().is_success() => {
            let detail = format!("http {}", res.status().as_u16());
            tracing::warn!("guard frontier replay rejected: {detail}");
            let event =
                guard::guard_event("fallback_failed", reason, &guarded_model, Some(&detail));
            guard_emit.note_guard(Some(&frontier), event);
            guard_emit.emit(Usage::default(), false, Outcome::Error, Some(ErrorClass::Guard));
            json_error(
                StatusCode::BAD_GATEWAY,
                "guard_fallback_error",
                "frontier fallback was rejected",
            )
        }
        Ok(res) => {
            // Note the event BEFORE buffering the replay so a mid-replay
            // client abort (Drop) still records the guard event (AC3).
            let event = guard::guard_event("fallback", reason, &guarded_model, None);
            guard_emit.note_guard(Some(&frontier), event);
            let bytes = match res.bytes().await {
                Ok(bytes) => bytes,
                Err(e) => {
                    tracing::error!("guard frontier body read failed: {e}");
                    guard_emit.emit(Usage::default(), true, Outcome::Truncated, Some(ErrorClass::Transport));
                    return json_error(
                        StatusCode::BAD_GATEWAY,
                        "guard_fallback_error",
                        "frontier fallback body read failed",
                    );
                }
            };
            let usage = usage_from_sse_bytes(&bytes).unwrap_or_default();
            guard_emit.emit(usage, false, outcome_for_sse(200, &bytes), None);
            let mut response = Response::new(Body::from(bytes));
            *response.status_mut() = StatusCode::OK;
            response
                .headers_mut()
                .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
            with_trace_id(response, guard_emit.meta.id)
        }
    }
}

/// Drop-guarded trace emitter for the buffered (non-streamed-to-client)
/// phase: if the client disconnects while the upstream body is still being
/// buffered the handler future is dropped, and only this guard's Drop runs —
/// emitting a truncated trace (ITEM-009 semantics preserved for the guard).
struct BufferedGuard {
    tx: mpsc::Sender<TraceMessage>,
    meta: RequestMeta,
    tenant_id: Uuid,
    started: Instant,
    emitted: Cell<bool>,
    /// ITEM-025: the model that ultimately served (the frontier replay),
    /// overriding `meta.model` on the request's trace — the cost basis.
    retarget_model: RefCell<Option<String>>,
    /// ITEM-025 AC3: guard event attached to the single trace once the guard
    /// has fired. None until `note_guard` runs — additive only.
    guard_event: RefCell<Option<Value>>,
}

impl BufferedGuard {
    fn new(
        tx: mpsc::Sender<TraceMessage>,
        meta: RequestMeta,
        tenant_id: Uuid,
        started: Instant,
    ) -> Self {
        Self {
            tx,
            meta,
            tenant_id,
            started,
            emitted: Cell::new(false),
            retarget_model: RefCell::new(None),
            guard_event: RefCell::new(None),
        }
    }

    /// ITEM-025/026: record that the guard fired on this request. `served` is
    /// the frontier model when a fallback replays it (retargets the trace's
    /// request_model to the model that ultimately serves — the cost basis),
    /// None when the cheap model itself served. Must run before any await
    /// after the guard verdict so a client abort still records the event.
    fn note_guard(&self, served: Option<&str>, event: Value) {
        *self.retarget_model.borrow_mut() = served.map(str::to_string);
        *self.guard_event.borrow_mut() = Some(event);
    }

    fn emit(&self, usage: Usage, truncated: bool, outcome: Outcome, error_class: Option<ErrorClass>) {
        if self.emitted.replace(true) {
            return;
        }
        let total_ms = self.started.elapsed().as_millis() as i64;
        let mut meta = self.meta.clone();
        if let Some(model) = self.retarget_model.borrow().clone() {
            meta.model = model;
        }
        let mut msg = build_trace(
            &meta,
            self.tenant_id,
            TraceOutcome {
                status: 200,
                total_duration_ms: total_ms,
                usage,
                ttft_ms: None,
                raw_completion: None,
                truncated,
                outcome,
                error_class,
            },
        );
        msg.guard_event = self.guard_event.borrow().clone();
        send_trace(&self.tx, msg);
    }
}

impl Drop for BufferedGuard {
    fn drop(&mut self) {
        if !self.emitted.get() {
            tracing::debug!("guarded request dropped before flush; recording truncated trace");
            self.emit(Usage::default(), true, Outcome::Truncated, Some(ErrorClass::ClientAbort));
        }
    }
}

/// One-shot trace emitter for the streaming path. Sends the trace exactly
/// once: on clean/errored completion from the stream loop, or from `Drop` when
/// the client aborts mid-stream — nothing after a dropped future's last poll
/// runs, so `Drop` is the only hook (ITEM-009). `send` is synchronous
/// (`try_send`), so dropping never blocks.
struct StreamEmit {
    tx: mpsc::Sender<TraceMessage>,
    meta: RequestMeta,
    tenant_id: Uuid,
    status: u16,
    started: Instant,
    emitted: Cell<bool>,
    ttft_ms: Cell<Option<i64>>,
}

impl StreamEmit {
    fn new(
        tx: mpsc::Sender<TraceMessage>,
        meta: RequestMeta,
        tenant_id: Uuid,
        status: u16,
        started: Instant,
    ) -> Self {
        Self {
            tx,
            meta,
            tenant_id,
            status,
            started,
            emitted: Cell::new(false),
            ttft_ms: Cell::new(None),
        }
    }

    fn note_ttft(&self, ttft_ms: i64) {
        self.ttft_ms.set(Some(ttft_ms));
    }

    fn ttft_ms(&self) -> Option<i64> {
        self.ttft_ms.get()
    }

    /// Emit the trace unless one was already sent. `truncated` marks a stream
    /// that ended before the upstream finished (mid-stream error).
    fn emit(&self, usage: Usage, truncated: bool, outcome: Outcome, error_class: Option<ErrorClass>) {
        if self.emitted.replace(true) {
            return;
        }
        let total_ms = self.started.elapsed().as_millis() as i64;
        let msg = build_trace(
            &self.meta,
            self.tenant_id,
            TraceOutcome {
                status: self.status,
                total_duration_ms: total_ms,
                usage,
                ttft_ms: self.ttft_ms(),
                raw_completion: None,
                truncated,
                outcome,
                error_class,
            },
        );
        send_trace(&self.tx, msg);
    }
}

impl Drop for StreamEmit {
    fn drop(&mut self) {
        if !self.emitted.get() {
            // Client dropped the response mid-stream: the usage frame (sent at
            // the very end by the provider) never arrived, so token counts are
            // unknown — record zeroes and flag the trace as truncated.
            tracing::debug!("stream dropped before completion; recording truncated trace");
            self.emit(Usage::default(), true, Outcome::Truncated, Some(ErrorClass::ClientAbort));
        }
    }
}

/// Tag each upstream chunk with its arrival time (TTFT on the first one), yield
/// it immediately, and once the stream ends queue the trace with token counts
/// parsed from the final usage frame. A client abort mid-stream still emits a
/// truncated trace via StreamEmit's Drop.
fn stream_with_trace(
    byte_stream: impl Stream<Item = Result<axum::body::Bytes, reqwest::Error>>,
    started: Instant,
    meta: RequestMeta,
    tenant_id: Uuid,
    status: u16,
    tx: mpsc::Sender<TraceMessage>,
    parse_usage: fn(&[u8]) -> Option<Usage>,
) -> impl Stream<Item = Result<axum::body::Bytes, std::io::Error>> {
    async_stream::stream! {
        let emit = StreamEmit::new(tx, meta, tenant_id, status, started);
        let mut stream = Box::pin(byte_stream);
        let mut accumulated: Vec<u8> = Vec::new();
        let mut errored = false;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    if emit.ttft_ms().is_none() {
                        emit.note_ttft(started.elapsed().as_millis() as i64);
                        tracing::debug!(ttft_ms = emit.ttft_ms().unwrap(), "first upstream chunk");
                    }
                    accumulated.extend_from_slice(&bytes);
                    yield Ok(bytes);
                }
                Err(e) => {
                    tracing::error!("upstream stream error: {e}");
                    errored = true;
                    break;
                }
            }
        }
        let usage = parse_usage(&accumulated).unwrap_or_default();
        let outcome = if errored {
            Outcome::Truncated
        } else {
            outcome_for_sse(status, &accumulated)
        };
        emit.emit(usage, errored, outcome, errored.then_some(ErrorClass::Transport));
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
    async fn a_tenant_provider_overrides_the_shared_key() {
        let tenant = seeded_tenant();
        let state = route_state(std::sync::Arc::new(
            crate::providers::InMemoryProviderStore::with_provider(
                tenant.id,
                "https://my-provider.test/v1",
                "sk-my-provider",
            ),
        ));
        let route = resolve_route(&state, &tenant, None, Surface::OpenAi).await.expect("route");
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
        let route = resolve_route(&state, &tenant, None, Surface::OpenAi).await.expect("route");
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
