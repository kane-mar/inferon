//! The data plane: the OpenAI-compatible and Anthropic Messages ingress.
//!
//! Everything between "a request arrived" and "a trace was queued": resolve the
//! provider, apply the router/budget/guard/affinity/policy controls, forward
//! upstream, and emit exactly one trace. `lib.rs` owns the composition root
//! (`AppState`, `build_router`) and the shared seams; this module owns the
//! request path.

use crate::errors::ErrorClass;
use crate::guard::GuardReason;
use crate::quality::{outcome_for_json, outcome_for_sse, Outcome};
use crate::usage::{
    system_prompt_hash, system_prompt_hash_anthropic, usage_from_anthropic_json,
    usage_from_anthropic_sse_bytes, usage_from_sse_bytes, usage_from_json, Usage,
};
use crate::{
    authenticate, budget, dashboard, guard, json_error, providers, routing, sha256_hex, AppState,
    Tenant, TraceMessage, MAX_REQUEST_BYTES,
};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header::CONTENT_TYPE, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::{Stream, StreamExt};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    sync::Arc,
    time::Instant,
};
use tokio::sync::mpsc;
use uuid::Uuid;

pub(crate) async fn healthz() -> &'static str {
    "ok"
}

/// GET /v1/models — the tenant's available models in the OpenAI list shape, so
/// SDKs can discover them. It's the union of the providers' advertised models
/// (cached), minus disabled ones; a tenant with nothing cached yet discovers
/// its default provider's list once.
pub(crate) async fn list_models(State(state): State<Arc<AppState>>, req: Request) -> Response {
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
pub(crate) struct Route {
    pub(crate) url: String,
    pub(crate) key: String,
    /// Extra upstream headers from the tenant's provider (ITEM-043 AC1).
    pub(crate) metadata: providers::ProviderMetadata,
    /// ITEM-047: the provider row that served the request (`None` = shared key).
    pub(crate) provider_id: Option<Uuid>,
    /// ITEM-050: the provider kind, which decides the upstream auth + surface.
    pub(crate) kind: String,
    /// ITEM-077: a locally hosted provider — costed at zero.
    pub(crate) is_local: bool,
}

/// The ingress→upstream surface (ITEM-050): OpenAI is the chat-completions
/// shape, Anthropic the Messages shape. A request never crosses surfaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Surface {
    OpenAi,
    Anthropic,
}

impl Surface {
    /// `POST <base>/chat/completions` or `POST <base>/messages`.
    pub(crate) fn endpoint(self, base_url: &str) -> String {
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

/// An upstream request with the provider's auth and metadata headers. A keyless
/// local provider sends no Authorization header at all (ITEM-082); every other
/// kind sends its credential (Anthropic on `x-api-key`).
fn provider_request(
    client: &reqwest::Client,
    route: &Route,
) -> reqwest::RequestBuilder {
    let mut request = client
        .post(&route.url)
        .header("Content-Type", "application/json");
    for (header, value) in providers::auth_headers(&route.kind, &route.key, &route.metadata) {
        request = request.header(header, value);
    }
    apply_provider_headers(request, &route.metadata)
}

/// The adaptive policy's health weight for a provider (1.0 = no opinion).
fn weight_of(weights: &std::collections::BTreeMap<String, f64>, id: &Uuid) -> f64 {
    weights.get(&id.to_string()).copied().unwrap_or(1.0)
}

pub(crate) async fn resolve_route(
    state: &Arc<AppState>,
    tenant: &Tenant,
    model: Option<&str>,
    surface: Surface,
    weights: Option<&std::collections::BTreeMap<String, f64>>,
) -> Result<Route, Response> {
    // ITEM-069: when several providers own the model, the adaptive policy's
    // health weights pick the healthiest; otherwise the store's own ordering.
    let preferred = match (model, weights) {
        (Some(model), Some(weights)) if !weights.is_empty() => state
            .provider_store
            .providers_for_model(tenant.id, model)
            .await
            .ok()
            .and_then(|ids| {
                ids.into_iter().max_by(|a, b| {
                    weight_of(weights, a)
                        .partial_cmp(&weight_of(weights, b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            }),
        _ => None,
    };
    let resolved = match preferred {
        Some(id) => state.provider_store.resolve_provider(tenant.id, id).await,
        None => state.provider_store.resolve_for(tenant.id, model).await,
    };
    match resolved {
        Ok(Some(provider)) => {
            // ITEM-050 AC5: never cross surfaces — an Anthropic provider serves
            // only /v1/messages, an OpenAI-shaped one only /v1/chat/completions.
            if providers::is_anthropic(&provider.kind) != (surface == Surface::Anthropic) {
                return Err(surface_mismatch(surface));
            }
            Ok(Route {
                url: surface.endpoint(&provider.base_url),
                is_local: providers::is_local(&provider.kind),
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
                    is_local: false,
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
    weights: Option<&std::collections::BTreeMap<String, f64>>,
) -> Vec<(String, Route)> {
    let mut seen = std::collections::HashSet::new();
    let mut chain = Vec::new();
    for model in std::iter::once(primary.clone()).chain(fallbacks) {
        if !seen.insert(model.clone()) {
            continue;
        }
        let decision = if model == primary {
            // The primary was already checked by `ensure_model_enabled`.
            crate::hotpath::Decision {
                route: route.clone(),
                disabled: false,
            }
        } else {
            state
                .hotpath
                .route(state, tenant, &model, Surface::OpenAi, weights)
                .await
                .unwrap_or_else(|_| crate::hotpath::Decision {
                    route: route.clone(),
                    disabled: false,
                })
        };
        if decision.disabled {
            continue;
        }
        chain.push((model, decision.route));
    }
    if chain.is_empty() {
        chain.push((primary, route.clone()));
    }
    chain
}

/// What the router decided for a request: the route for the client's model and
/// the ordered chain to try. Fills in the trace-visible decisions on `meta`
/// (provider, router event, cache affinity) as it goes.
struct ModelPlan {
    route: Route,
    /// The model the chain starts from (after any override/affinity) — recorded
    /// as the `from_model` of a fallback event.
    primary: String,
    chain: Vec<(String, Route)>,
    /// ITEM-091: the tenant's pruning policy, read with the rest of the policy.
    prune: Option<crate::prune::PrunePolicy>,
}

/// Turn the request into a model plan: read the adaptive policy, resolve the
/// route, apply the budget breaker and the heuristic router, prefer a warm
/// cache, then append the tenant's fallback chain — dropping models the tenant
/// disabled. Trace-visible decisions (provider, router event, cache affinity)
/// are recorded on `meta` as they are made.
async fn plan_models(
    state: &Arc<AppState>,
    tenant: &Tenant,
    meta: &mut RequestMeta,
    started: Instant,
) -> Result<ModelPlan, Response> {
    let tenant_id = tenant.id;
    // ITEM-064/069: read the adaptive policy once — it supplies both the router
    // threshold and the provider health weights.
    let policy = state.hotpath.policy(state, tenant_id).await;
    meta.policy_version = policy.as_ref().map(|policy| policy.version);
    let policy_weights = policy.as_ref().map(|policy| &policy.provider_weights);
    let decision = state
        .hotpath
        .route(state, tenant, &meta.model, Surface::OpenAi, policy_weights)
        .await?;
    let route = decision.route;
    meta.provider_id = route.provider_id;
    meta.is_local = route.is_local;
    crate::metrics::request_routed(route.provider_id);

    // ITEM-044 AC5: a model the tenant disabled is refused with a clear typed
    // error (and traced), not a raw upstream 4xx.
    ensure_model_enabled(state, meta, tenant_id, started, decision.disabled)?;

    // ITEM-016: the budget breaker wins over the router and rejects before any
    // upstream call; ITEM-046/081: otherwise the router may downshift.
    let policy_threshold = policy.as_ref().and_then(|policy| policy.router_threshold);
    let chosen = match budget::check_budget(state, tenant_id, &meta.raw_prompt, policy_threshold)
        .await
    {
        Ok(chosen) => chosen,
        Err(resp) => {
            trace_error(state, meta, tenant_id, started, 429, ErrorClass::Inferon4xx);
            return Err(resp);
        }
    };
    let primary = override_primary(state, route.provider_id, meta, chosen).await;
    // ITEM-063: prefer a warm provider cache over a marginally cheaper miss.
    let primary = prefer_warm(state, tenant_id, meta, primary).await;
    let fallbacks = state.hotpath.fallbacks(state, tenant_id).await;
    let chain =
        build_model_chain(state, tenant, &route, primary.clone(), fallbacks, policy_weights).await;
    Ok(ModelPlan {
        route,
        primary,
        chain,
        prune: policy.as_ref().and_then(|policy| policy.prune.clone()),
    })
}

/// ITEM-044 AC4: never select a model the tenant disabled — the requested model
/// then stands. Otherwise the override becomes the primary and, when the
/// heuristic router caused it, is recorded on the trace (ITEM-054/081).
async fn override_primary(
    state: &Arc<AppState>,
    provider_id: Option<Uuid>,
    meta: &mut RequestMeta,
    chosen: Option<budget::ModelOverride>,
) -> String {
    let Some(chosen) = chosen else {
        return meta.model.clone();
    };
    if state
        .model_store
        .is_disabled(provider_id, &chosen.model)
        .await
        .unwrap_or(false)
    {
        return meta.model.clone();
    }
    if chosen.reason == budget::ROUTER_REASON {
        let mut event = json!({
            "from_model": meta.model,
            "to_model": chosen.model,
            "reason": chosen.reason,
        });
        if let Some(source) = chosen.pair_source {
            event["pair_source"] = json!(source.label());
        }
        meta.router_event = Some(event);
    }
    chosen.model
}

/// ITEM-063: keep a warm provider cache over a marginally cheaper miss.
async fn prefer_warm(
    state: &Arc<AppState>,
    tenant_id: Uuid,
    meta: &mut RequestMeta,
    primary: String,
) -> String {
    let Some(hash) = meta.system_prompt_hash.as_deref() else {
        return primary;
    };
    match state.affinity_store.warm_target(tenant_id, hash).await {
        Ok(Some(warm))
            if warm.model != primary && crate::affinity::keep_warm(&warm.model, &primary) =>
        {
            meta.cache_affinity = Some(json!({
                "from_model": primary,
                "to_model": warm.model,
                "reason": "cache_affinity",
            }));
            warm.model
        }
        _ => primary,
    }
}


/// Proxy a chat completion request to the configured upstream.
///
/// Authenticates the caller via `Authorization: Bearer <key>` (SHA-256 lookup
/// against the tenant store), swaps the upstream auth header to the tenant's
/// master key, then forwards: streaming requests pipe upstream bytes through
/// unchanged (zero-copy) as text/event-stream; non-streaming requests pass the
/// JSON body through with usage intact. Transport errors become 502; upstream
/// HTTP error statuses are forwarded as-is.
pub(crate) async fn proxy_chat_completions(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Response {
    let _in_flight = crate::metrics::request_started();
    // One clock for the whole request: `started` is both the trace's duration
    // basis and the split point for the proxy's own pre-upstream time.
    let started = Instant::now();
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
    // ITEM-092: stage timing at debug level — auth, then planning (config, route,
    // budget/router), then the total pre-dispatch overhead set below.
    let auth_us = started.elapsed().as_micros() as i64;
    let session_id = crate::session::parse_id(
        req.headers()
            .get(crate::session::SESSION_ID_HEADER)
            .and_then(|value| value.to_str().ok()),
    );

    let (_, payload) = match read_json_body(req).await {
        Ok(parsed) => parsed,
        Err(resp) => return resp,
    };
    let is_stream = payload["stream"].as_bool().unwrap_or(false);
    // The router/budget override is decided per request in `plan_models` below,
    // against the tenant's config (ITEM-045/046).
    let mut meta = RequestMeta {
        id: Uuid::new_v4(),
        model: payload["model"].as_str().unwrap_or("unknown").to_string(),
        system_prompt_hash: system_prompt_hash(&payload),
        raw_prompt: payload,
        routing_event: None,
        router_event: None,
        cache_affinity: None,
        policy_version: None,
        proxy_ms: None,
        proxy_us: None,
        wire_format: None,
        provider_id: None,
        api_key_id: tenant.api_key_id,
        is_local: false,
        capture_payloads: true,
        session_id,
        lint: None,
        prune_event: None,
    };
    // ITEM-088: the tenant's consent decides whether payloads are stored at all.
    meta.capture_payloads = state.hotpath.capture_enabled(&state, tenant_id).await;
    preflight(&state, tenant_id, &mut meta).await;
    let plan_started = Instant::now();
    let ModelPlan {
        route,
        primary,
        chain: allowed,
        prune,
    } = match plan_models(&state, &tenant, &mut meta, started).await {
        Ok(plan) => plan,
        Err(resp) => return resp,
    };

    // ITEM-091: rewrite the payload when the tenant opted in and this request is
    // not part of the holdout. The rewrite is attributed on the trace.
    prune_payload(&mut meta, prune.as_ref(), &tenant_id.to_string());

    let mut routing_event: Option<Value> = None;
    // ITEM-066: the proxy's own pre-upstream time, recorded before the first
    // dispatch (a subset of the trace's total duration).
    meta.proxy_ms = Some(started.elapsed().as_millis() as i64);
    meta.proxy_us = Some(started.elapsed().as_micros() as i64);
    tracing::debug!(
        auth_us,
        plan_us = plan_started.elapsed().as_micros() as i64,
        pre_dispatch_us = meta.proxy_us.unwrap_or_default(),
        "hot-path stages"
    );
    let mut upstream: Option<reqwest::Response> = None;
    for index in 0..allowed.len() {
        let (model, attempt_route) = (&allowed[index].0, &allowed[index].1);
        let model = model.clone();
        // ITEM-067: the target's wire-format capability is per attempt, so a
        // fallback to a provider that honours json_schema keeps the contract.
        let downgrade_schema = guard::request_schema(&meta.raw_prompt).is_some()
            && !providers::supports_json_schema(&attempt_route.kind, &model);
        let body = match budget::forward_body(&meta.raw_prompt, &model, downgrade_schema) {
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
        meta.wire_format = downgrade_schema.then_some("json_object");
        let attempt = provider_request(&state.http_client, attempt_route)
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
            && state.hotpath.guard_enabled(&state, tenant_id).await;
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
pub(crate) async fn proxy_messages(State(state): State<Arc<AppState>>, req: Request) -> Response {
    let _in_flight = crate::metrics::request_started();
    let request_started = Instant::now();
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
    let session_id = crate::session::parse_id(
        req.headers()
            .get(crate::session::SESSION_ID_HEADER)
            .and_then(|value| value.to_str().ok()),
    );

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
        proxy_ms: None,
        proxy_us: None,
        wire_format: None,
        provider_id: None,
        api_key_id: tenant.api_key_id,
        is_local: false,
        capture_payloads: true,
        session_id,
        lint: None,
        prune_event: None,
    };
    meta.capture_payloads = state.hotpath.capture_enabled(&state, tenant_id).await;
    preflight(&state, tenant_id, &mut meta).await;
    let policy = state.hotpath.policy(&state, tenant_id).await;
    meta.policy_version = policy.as_ref().map(|policy| policy.version);
    prune_payload(
        &mut meta,
        policy.as_ref().and_then(|policy| policy.prune.as_ref()),
        &tenant_id.to_string(),
    );
    let decision = match state
        .hotpath
        .route(&state, &tenant, &meta.model, Surface::Anthropic, None)
        .await
    {
        Ok(decision) => decision,
        Err(resp) => return resp,
    };
    let route = decision.route;
    meta.provider_id = route.provider_id;
    meta.is_local = route.is_local;
    crate::metrics::request_routed(route.provider_id);
    let started = Instant::now();

    if let Err(resp) = ensure_model_enabled(&state, &meta, tenant_id, started, decision.disabled) {
        return resp;
    }

    // ITEM-050 AC5: the heuristic router and cheap-fallback are OpenAI-shaped
    // and bypassed here; only the hard monthly cap still applies.
    if let Err(resp) = budget::check_cap(&state, tenant_id).await {
        trace_error(&state, &meta, tenant_id, started, 429, ErrorClass::Inferon4xx);
        return resp;
    }

    // ITEM-066: the proxy's own pre-upstream time, recorded before dispatch.
    meta.proxy_ms = Some(request_started.elapsed().as_millis() as i64);
    meta.proxy_us = Some(request_started.elapsed().as_micros() as i64);
    let request = provider_request(&state.http_client, &route);
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
    state: &Arc<AppState>,
    headers: &axum::http::HeaderMap,
) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>> {
    let key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .filter(|key| !key.is_empty());
    match key {
        Some(key) => state.hotpath.tenant_for_key(state, &sha256_hex(key)).await,
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
            raw_completion_capped: false,
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
    with_lint(with_trace_id(response, meta.id), meta.lint.as_ref())
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
    /// ITEM-066/092: the proxy's own pre-upstream time (request-received →
    /// dispatch), in ms for the dashboard and µs for the performance suite.
    proxy_ms: Option<i64>,
    proxy_us: Option<i64>,
    /// ITEM-067: set when the wire format was downgraded (json_schema →
    /// json_object) because the target cannot honour a schema contract.
    wire_format: Option<&'static str>,
    /// ITEM-047: provider + API key attribution for the trace.
    provider_id: Option<Uuid>,
    api_key_id: Option<Uuid>,
    /// ITEM-077: served by a locally hosted provider — costed at zero.
    is_local: bool,
    /// ITEM-088: whether raw payloads may be stored for this tenant.
    capture_payloads: bool,
    /// ITEM-093: the client's transaction id, when it supplied a valid one.
    session_id: Option<String>,
    /// ITEM-090: pre-flight payload findings (None = nothing fired).
    lint: Option<Value>,
    /// ITEM-091: what pruning removed (or that this request was held out).
    prune_event: Option<Value>,
}

/// The outcome of a proxied request, as recorded on its trace.
struct TraceOutcome {
    status: u16,
    total_duration_ms: i64,
    usage: Usage,
    ttft_ms: Option<i64>,
    raw_completion: Option<Value>,
    /// ITEM-085: the capture hit the size cap (a head, not a full pair).
    raw_completion_capped: bool,
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
            raw_completion_capped: false,
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
        raw_prompt: meta.capture_payloads.then(|| meta.raw_prompt.clone()),
        raw_completion: if meta.capture_payloads { outcome.raw_completion } else { None },
        raw_completion_capped: meta.capture_payloads && outcome.raw_completion_capped,
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
        proxy_ms: meta.proxy_ms,
        proxy_us: meta.proxy_us,
        example_state: None,
        example_reason: None,
        completion_hash: None,
        session_id: meta.session_id.clone(),
        request_lint: meta.lint.clone(),
        prune_event: meta.prune_event.clone(),
        is_local: meta.is_local,
    }
}

/// Refuse a model the tenant disabled with a typed, traced 400 (ITEM-044 AC5).
fn ensure_model_enabled(
    state: &AppState,
    meta: &RequestMeta,
    tenant_id: Uuid,
    started: Instant,
    disabled: bool,
) -> Result<(), Response> {
    if !disabled {
        return Ok(());
    }
    trace_error(state, meta, tenant_id, started, 400, ErrorClass::Inferon4xx);
    Err(json_error(
        StatusCode::BAD_REQUEST,
        "model_disabled",
        "this model is disabled for your tenant",
    ))
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
pub(crate) struct QualitySignalBody {
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
pub(crate) async fn quality_signal(
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

/// Body of `POST /v1/lint/dismiss` (ITEM-090).
#[derive(serde::Deserialize)]
pub(crate) struct DismissLintBody {
    trace_id: Uuid,
}

/// POST /v1/lint/dismiss — the tenant judged a finding a false positive.
///
/// Tenant-authenticated with the tenant's API key; the trace must carry a
/// finding and belong to the caller. Dismissed findings stay on the trace and
/// stop counting as actionable on the performance page.
pub(crate) async fn dismiss_lint(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<DismissLintBody>,
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
            tracing::error!("lint dismissal tenant lookup failed: {e}");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "tenant_store_error",
                "tenant lookup failed",
            );
        }
    };
    let updated = sqlx::query(
        "UPDATE traces SET lint_dismissed_at = NOW()
         WHERE id = $1 AND tenant_id = $2 AND request_lint IS NOT NULL",
    )
    .bind(body.trace_id)
    .bind(tenant.id)
    .execute(&state.db)
    .await;
    match updated {
        Ok(result) if result.rows_affected() == 0 => json_error(
            StatusCode::NOT_FOUND,
            "no_finding",
            "no lint finding for that trace",
        ),
        Ok(_) => (
            StatusCode::ACCEPTED,
            Json(json!({ "ok": true, "trace_id": body.trace_id.to_string() })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("lint dismissal failed: {e}");
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "lint_store_error",
                "could not dismiss the finding",
            )
        }
    }
}

/// ITEM-090: inspect the payload before dispatch. Advisory only — nothing is
/// rewritten; the finding rides the trace and `X-Inferon-Lint`. Skipped when the
/// tenant turned the switch off, and a failed lookup never fails the request.
async fn preflight(state: &Arc<AppState>, tenant_id: Uuid, meta: &mut RequestMeta) {
    if !state.hotpath.lint_enabled(state, tenant_id).await {
        return;
    }
    // The repeat check compares this turn's system prompt with the tenant's
    // previous one; the rest of the pass is pure.
    let repeated = meta.system_prompt_hash.as_ref().is_some_and(|hash| {
        state
            .hotpath
            .note_prompt(tenant_id, hash, crate::preflight::REPEAT_WINDOW)
    });
    meta.lint = crate::preflight::inspect(&meta.raw_prompt, repeated, &meta.model);
}

/// ITEM-091: apply the tenant's pruning policy, holding out a deterministic
/// share unpruned so the effect is measurable.
///
/// A rewrite that produces nothing (disabled policy, nothing redundant) leaves
/// the payload untouched; a rewrite never fails the request. Held-out requests
/// still record an event, so the dashboard can compare the two groups.
fn prune_payload(
    meta: &mut RequestMeta,
    policy: Option<&crate::prune::PrunePolicy>,
    tenant_id: &str,
) {
    let Some(policy) = policy.filter(|policy| policy.enabled) else {
        return;
    };
    let seed = format!(
        "{tenant_id}:{}",
        meta.system_prompt_hash.as_deref().unwrap_or_default()
    );
    if crate::prune::in_holdout(&seed, policy) {
        meta.prune_event = Some(json!({
            "policy_version": meta.policy_version,
            "held_out": true,
            "turns_removed": 0,
        }));
        return;
    }
    let Some(rewrite) = crate::prune::rewrite(&meta.raw_prompt, policy) else {
        return;
    };
    let mut event = rewrite.event(&meta.model, meta.policy_version.unwrap_or(0));
    event["held_out"] = json!(false);
    meta.prune_event = Some(event);
    meta.raw_prompt = rewrite.payload;
}

/// ITEM-090: put the finding kinds on the response (`X-Inferon-Lint`), so the
/// client can see them without opening the dashboard.
fn with_lint(mut response: Response, lint: Option<&Value>) -> Response {
    let Some(kinds) = lint.and_then(|lint| lint["kinds"].as_array()) else {
        return response;
    };
    let joined = kinds
        .iter()
        .filter_map(|kind| kind.as_str())
        .collect::<Vec<_>>()
        .join(",");
    if let Ok(value) = HeaderValue::from_str(&joined) {
        response.headers_mut().insert("x-inferon-lint", value);
    }
    response
}

/// ITEM-055: tag a response with the request's trace id so a client can attach
/// a quality signal to it out of band.
fn with_trace_id(mut response: Response, trace_id: Uuid) -> Response {
    if let Ok(value) = HeaderValue::from_str(&trace_id.to_string()) {
        response.headers_mut().insert("x-inferon-trace-id", value);
    }
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
    let lint = meta.lint.clone();
    let stream = stream_with_trace(byte_stream, started, meta, tenant_id, status, tx, parse_usage);
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    with_lint(with_trace_id(response, trace_id), lint.as_ref())
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
            guard_emit.note_completion(&bytes);
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
            with_lint(with_trace_id(response, guard_emit.meta.id), guard_emit.meta.lint.as_ref())
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
    // ITEM-067: the replay to the frontier is normalized the same way — a single
    // DeepSeek deployment cannot accept json_schema at all.
    let downgrade = guard::request_schema(&guard_emit.meta.raw_prompt).is_some()
        && !providers::supports_json_schema(&route.kind, &frontier);
    let replay_body = match budget::forward_body(&guard_emit.meta.raw_prompt, &frontier, downgrade) {
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
    let replay = provider_request(&state.http_client, route)
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
            // ITEM-067 AC4: never serve an invalid replay silently — record a
            // `fallback_invalid` event when the frontier also violates the schema.
            let schema = guard::request_schema(&guard_emit.meta.raw_prompt);
            if guard::check_guarded_sse(&bytes, schema.as_ref()).is_err() {
                guard_emit.note_guard(
                    Some(&frontier),
                    guard::guard_event("fallback_invalid", reason, &guarded_model, None),
                );
            }
            guard_emit.emit(usage, false, outcome_for_sse(200, &bytes), None);
            let mut response = Response::new(Body::from(bytes));
            *response.status_mut() = StatusCode::OK;
            response
                .headers_mut()
                .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
            with_lint(with_trace_id(response, guard_emit.meta.id), guard_emit.meta.lint.as_ref())
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
    /// ITEM-085: the buffered completion, for the dataset pair.
    completion: RefCell<Option<crate::capture::Completion>>,
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
            completion: RefCell::new(None),
        }
    }

    /// ITEM-025/026: record that the guard fired on this request. `served` is
    /// the frontier model when a fallback replays it (retargets the trace's
    /// request_model to the model that ultimately serves — the cost basis),
    /// None when the cheap model itself served. Must run before any await
    /// after the guard verdict so a client abort still records the event.
    fn note_guard(&self, served: Option<&str>, mut event: Value) {
        // ITEM-067: surface a downgraded wire format on the guard event.
        if let Some(wire_format) = self.meta.wire_format {
            event["wire_format"] = json!(wire_format);
        }
        *self.retarget_model.borrow_mut() = served.map(str::to_string);
        *self.guard_event.borrow_mut() = Some(event);
    }

    /// ITEM-085: the buffered body is a whole completion; keep it for the pair.
    fn note_completion(&self, bytes: &[u8]) {
        *self.completion.borrow_mut() = crate::capture::from_sse_bytes(bytes);
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
                raw_completion: self
                    .completion
                    .borrow()
                    .as_ref()
                    .map(|captured| captured.value.clone()),
                raw_completion_capped: self
                    .completion
                    .borrow()
                    .as_ref()
                    .is_some_and(|captured| captured.capped),
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
    /// Upstream bytes seen so far, for usage and (ITEM-085) the completion.
    raw: RefCell<Vec<u8>>,
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
            raw: RefCell::new(Vec::new()),
        }
    }

    /// Keep the upstream bytes for the completion capture (ITEM-085). The stream
    /// loop already accumulates them for usage, so this is the same buffer.
    fn note_chunk(&self, bytes: &[u8]) {
        self.raw.borrow_mut().extend_from_slice(bytes);
    }

    fn completion(&self) -> Option<crate::capture::Completion> {
        crate::capture::from_sse_bytes(&self.raw.borrow())
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
        let completion = self.completion();
        let msg = build_trace(
            &self.meta,
            self.tenant_id,
            TraceOutcome {
                status: self.status,
                total_duration_ms: total_ms,
                usage,
                ttft_ms: self.ttft_ms(),
                raw_completion: completion.as_ref().map(|captured| captured.value.clone()),
                raw_completion_capped: completion.is_some_and(|captured| captured.capped),
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
        let mut errored = false;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    if emit.ttft_ms().is_none() {
                        emit.note_ttft(started.elapsed().as_millis() as i64);
                        tracing::debug!(ttft_ms = emit.ttft_ms().unwrap(), "first upstream chunk");
                    }
                    emit.note_chunk(&bytes);
                    yield Ok(bytes);
                }
                Err(e) => {
                    tracing::error!("upstream stream error: {e}");
                    errored = true;
                    break;
                }
            }
        }
        let accumulated = emit.raw.borrow().clone();
        let usage = parse_usage(&accumulated).unwrap_or_default();
        let outcome = if errored {
            Outcome::Truncated
        } else {
            outcome_for_sse(status, &accumulated)
        };
        emit.emit(usage, errored, outcome, errored.then_some(ErrorClass::Transport));
    }
}
