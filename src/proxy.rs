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

mod response;
mod route;
mod trace;

// The handlers live here; their pieces are re-exported so every existing
// `crate::proxy::X` path keeps working.
pub(crate) use response::*;
pub(crate) use route::*;
pub(crate) use trace::*;

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
            last_check_ok: None,
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
    let mut stages = crate::stages::Stages::new();
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
    // ITEM-092: named stage timing, logged at debug level — a regression says
    // which stage, not just "the proxy got slower".
    stages.mark("auth");
    let session_id = crate::session::parse_id(
        req.headers()
            .get(crate::session::SESSION_ID_HEADER)
            .and_then(|value| value.to_str().ok()),
    );

    let (_, payload) = match read_json_body(req).await {
        Ok(parsed) => parsed,
        Err(resp) => return resp,
    };
    stages.mark("body");
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
        pii: PiiPlan::default(),
        provider_selection: None,
        translate_to: None,
        rate_limit: None,
    };
    // ITEM-088: the tenant's consent decides whether payloads are stored at all.
    meta.capture_payloads = state.hotpath.capture_enabled(&state, tenant_id).await;
    meta.pii = state.hotpath.pii_plan(&state, tenant_id).await;
    preflight(&state, tenant_id, &mut meta).await;
    // ITEM-095: velocity first — a rejected request costs nothing and never
    // reaches the budget breaker.
    if let Err(resp) = enforce_rate_limit(&state, &tenant, &mut meta).await {
        trace_error(
            &state,
            &meta,
            tenant_id,
            started,
            429,
            ErrorClass::RateLimited,
        );
        return resp;
    }
    let ModelPlan {
        route,
        primary,
        chain: allowed,
        prune,
    } = match plan_models(&state, &tenant, &mut meta, started, &mut stages).await {
        Ok(plan) => plan,
        Err(resp) => return resp,
    };

    // ITEM-091: rewrite the payload when the tenant opted in and this request is
    // not part of the holdout. The rewrite is attributed on the trace.
    prune_payload(&mut meta, prune.as_ref(), &tenant_id.to_string());
    // ITEM-096: the wire copy, after pruning so the counts describe what is
    // actually sent. A deterministic share is held out to measure the effect.
    redact_wire(&mut meta, tenant_id);

    let mut routing_event: Option<Value> = None;
    // ITEM-066: the proxy's own pre-upstream time, recorded before the first
    // dispatch (a subset of the trace's total duration).
    stages.mark("prune");
    meta.proxy_ms = Some(started.elapsed().as_millis() as i64);
    meta.proxy_us = Some(started.elapsed().as_micros() as i64);
    tracing::debug!(
        stages = %stages.summary(),
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
        let body = match wire_body(&meta.raw_prompt, attempt_route, &model, downgrade_schema) {
            Ok(bytes) => bytes,
            Err(resp) => return resp,
        };
        meta.wire_format = downgrade_schema.then_some("json_object");
        // ITEM-100: count the dispatch, not the whole stream; the gauge is what
        // the operator sees while the provider is working.
        let attempt = {
            let _in_flight = crate::metrics::provider_started(meta.provider_id);
            provider_request(&state.http_client, attempt_route)
                .body(body)
                .send()
                .await
        };
        let is_last = index + 1 == allowed.len();
        match attempt {
            Ok(response) => {
                let status = response.status();
                let reason = routing::fallback_reason(status.as_u16());
                if status.is_success() || is_last || reason.is_none() {
                    meta.model = model;
                    meta.translate_to = attempt_route.translate;
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
        pii: PiiPlan::default(),
        provider_selection: None,
        translate_to: None,
        rate_limit: None,
    };
    meta.capture_payloads = state.hotpath.capture_enabled(&state, tenant_id).await;
    meta.pii = state.hotpath.pii_plan(&state, tenant_id).await;
    preflight(&state, tenant_id, &mut meta).await;
    let policy = state.hotpath.policy(&state, tenant_id).await;
    meta.policy_version = policy.as_ref().map(|policy| policy.version);
    prune_payload(
        &mut meta,
        policy.as_ref().and_then(|policy| policy.prune.as_ref()),
        &tenant_id.to_string(),
    );
    redact_wire(&mut meta, tenant_id);
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
    meta.provider_selection = route.selection;
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
    let wire = match route.translate {
        // Translated: the body is rebuilt in the provider's shape.
        Some(_) => match wire_body(&meta.raw_prompt, &route, &meta.model, false) {
            Ok(bytes) => bytes,
            Err(resp) => return resp,
        },
        // Native Anthropic: forward the caller's bytes unchanged.
        None => body_bytes,
    };
    meta.translate_to = route.translate;
    if let Err(resp) = enforce_rate_limit(&state, &tenant, &mut meta).await {
        trace_error(
            &state,
            &meta,
            tenant_id,
            started,
            429,
            ErrorClass::RateLimited,
        );
        return resp;
    }
    let upstream = match request.body(wire).send().await {
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
         WHERE id = $1 AND tenant_id = $2
           AND jsonb_array_length(request_lint->'kinds') > 0",
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

