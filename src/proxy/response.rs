use super::*;

/// Buffer an upstream body, trace the completed request, and build the
/// forwarded response. `content_type` overrides the forwarded content-type
/// when present; otherwise the upstream's own header is preserved (upstream
/// error bodies are forwarded as-is). Shared by the non-2xx and the 2xx
/// non-streaming paths.
pub(super) async fn forward_buffered(
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
    // ITEM-094: the trace (usage, outcome) is read from the provider's own
    // shape; the client gets the shape it asked for — the opposite surface.
    let bytes = match meta.translate_to {
        Some(Surface::OpenAi) => translated_body(&bytes, Surface::Anthropic).unwrap_or(bytes),
        Some(Surface::Anthropic) => translated_body(&bytes, Surface::OpenAi).unwrap_or(bytes),
        None => bytes,
    };
    let usage = parse_usage(&response_json);
    let outcome = outcome_for_json(status.as_u16(), &response_json);
    let error_class =
        (!status.is_success()).then(|| ErrorClass::from_upstream_status(status.as_u16()));
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
    decorate(response, meta)
}

/// ITEM-094: translate a buffered upstream body to the client surface. Errors
/// are translated too, so a provider failure arrives in the caller's shape.
pub(super) fn translated_body(
    bytes: &axum::body::Bytes,
    target: Surface,
) -> Option<axum::body::Bytes> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let translated = if value.get("error").is_some() && value.get("choices").is_none() {
        match target {
            Surface::Anthropic => crate::translate::error_to_anthropic(&value),
            Surface::OpenAi => crate::translate::error_to_openai(&value),
        }
    } else {
        match target {
            Surface::Anthropic => crate::translate::response_to_anthropic(&value).ok()?,
            Surface::OpenAi => crate::translate::response_to_openai(&value).ok()?,
        }
    };
    Some(axum::body::Bytes::from(translated.to_string()))
}

/// ITEM-095: charge this request against the key's velocity window and refuse
/// it when a limit is exceeded.
///
/// Pre-flight and pre-budget: a rejected request consumes no spend. Keys with no
/// limits (the default) touch no database at all; a counter failure fails open.
pub(super) async fn enforce_rate_limit(
    state: &Arc<AppState>,
    tenant: &Tenant,
    meta: &mut RequestMeta,
) -> Result<(), Response> {
    let Some(key_id) = tenant.api_key_id else {
        return Ok(());
    };
    // A key may set its own limits; otherwise the tenant default applies.
    let limits = if tenant.limits.is_unlimited() {
        match state.hotpath.budget(state, tenant.id).await {
            Ok(Some(budget)) => crate::ratelimit::Limits {
                requests_per_minute: budget.rpm_limit,
                tokens_per_minute: budget.tpm_limit,
            },
            _ => crate::ratelimit::Limits::default(),
        }
    } else {
        tenant.limits
    };
    if limits.is_unlimited() {
        return Ok(());
    }

    let estimate = crate::ratelimit::estimate_tokens(meta.raw_prompt.to_string().len());
    let Some(usage) = crate::ratelimit::charge(&state.db, key_id, estimate).await else {
        return Ok(()); // the limiter is down; the data plane keeps working
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    match crate::ratelimit::decide(limits, usage, crate::ratelimit::retry_after_secs(now)) {
        crate::ratelimit::Decision::Allowed(usage) => {
            meta.rate_limit = Some((limits, usage));
            Ok(())
        }
        crate::ratelimit::Decision::Limited {
            axis,
            limit,
            retry_after_secs,
        } => {
            tracing::warn!(
                key_id = %key_id, axis = axis.as_str(), limit,
                "rate limit exceeded"
            );
            Err(rate_limited_response(axis, limit, retry_after_secs))
        }
    }
}

/// ITEM-095: the 429 a limited key receives — typed body plus the standard
/// rate-limit headers.
pub(crate) fn rate_limited_response(
    axis: crate::ratelimit::Axis,
    limit: i64,
    retry_after_secs: i64,
) -> Response {
    let mut response = json_error(
        StatusCode::TOO_MANY_REQUESTS,
        "rate_limited",
        &format!("{} per minute exceeded for this key", axis.as_str()),
    );
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&retry_after_secs.to_string()) {
        headers.insert("retry-after", value);
    }
    if let Ok(value) = HeaderValue::from_str(&limit.to_string()) {
        headers.insert("x-ratelimit-limit", value);
    }
    headers.insert("x-ratelimit-remaining", HeaderValue::from_static("0"));
    headers.insert(
        "x-ratelimit-reset",
        HeaderValue::from_str(&retry_after_secs.to_string())
            .unwrap_or_else(|_| HeaderValue::from_static("60")),
    );
    response
}

/// Everything a proxied response carries back to the client: the trace id, the
/// pre-flight findings and the rate-limit window. One place, so a new header
/// cannot be forgotten on one of the three response paths.
pub(super) fn decorate(response: Response, meta: &RequestMeta) -> Response {
    let response = with_trace_id(response, meta.id);
    let response = with_lint(response, meta.lint.as_ref());
    with_rate_limit(response, meta)
}

/// ITEM-095: advertise the window on any response the key produced.
pub(super) fn with_rate_limit(mut response: Response, meta: &RequestMeta) -> Response {
    let Some((limits, usage)) = meta.rate_limit else {
        return response;
    };
    let headers = response.headers_mut();
    let limit = if limits.requests_per_minute > 0 {
        limits.requests_per_minute
    } else {
        limits.tokens_per_minute
    };
    let used = if limits.requests_per_minute > 0 {
        usage.requests
    } else {
        usage.tokens
    };
    if let Ok(value) = HeaderValue::from_str(&limit.to_string()) {
        headers.insert("x-ratelimit-limit", value);
    }
    if let Ok(value) = HeaderValue::from_str(&(limit - used).max(0).to_string()) {
        headers.insert("x-ratelimit-remaining", value);
    }
    if let Ok(value) = HeaderValue::from_str("60") {
        headers.insert("x-ratelimit-reset", value);
    }
    response
}

/// ITEM-094: the body actually sent, translated when the provider speaks the
/// other surface. Routing, the guard, pruning and the pre-flight lint all run on
/// the *client's* shape; translation is the last step before the wire.
pub(super) fn wire_body(
    payload: &Value,
    route: &Route,
    model: &str,
    downgrade_schema: bool,
) -> Result<axum::body::Bytes, Response> {
    let Some(target) = route.translate else {
        return budget::forward_body(payload, model, downgrade_schema)
            .map(axum::body::Bytes::from)
            .map_err(|e| {
                tracing::error!("payload reserialization failed: {e}");
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "serialization_error",
                    "failed to reserialize the request payload",
                )
            });
    };
    let translated = match target {
        Surface::OpenAi => crate::translate::request_to_openai(payload),
        Surface::Anthropic => crate::translate::request_to_anthropic(payload),
    };
    let mut translated = match translated {
        Ok(value) => value,
        Err(missing) => {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "surface_translation_unsupported",
                &format!("{}: {}", missing.field, missing.detail),
            ))
        }
    };
    // The router may have chosen a different model than the client asked for;
    // the wire model is the one we are actually calling.
    translated["model"] = json!(model);
    if downgrade_schema {
        // The structured-output normalization is OpenAI-only (ITEM-067).
        if let Some(schema) = translated.get("response_format").cloned() {
            translated["response_format"] = json!({"type": "json_object"});
            translated["_inferon_schema"] = schema;
        }
    }
    serde_json::to_vec(&translated)
        .map(axum::body::Bytes::from)
        .map_err(|e| {
            tracing::error!("translated payload serialization failed: {e}");
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "serialization_error",
                "failed to serialize the translated request payload",
            )
        })
}

/// ITEM-090: inspect the payload before dispatch. Advisory only — nothing is
/// rewritten; the finding rides the trace and `X-Inferon-Lint`. Skipped when the
/// tenant turned the switch off, and a failed lookup never fails the request.
pub(super) async fn preflight(state: &Arc<AppState>, tenant_id: Uuid, meta: &mut RequestMeta) {
    if !state.hotpath.lint_enabled(state, tenant_id).await {
        return;
    }
    let rules = state.hotpath.lint_rules(state, tenant_id).await;
    // The repeat check compares this turn's system prompt with the tenant's
    // previous one; the rest of the pass is pure.
    let repeated = meta.system_prompt_hash.as_ref().is_some_and(|hash| {
        state
            .hotpath
            .note_prompt(tenant_id, hash, crate::preflight::REPEAT_WINDOW)
    });
    // Every scanned request records a result, so "nothing found" is visible.
    meta.lint = Some(crate::preflight::inspect(
        &meta.raw_prompt,
        repeated,
        &meta.model,
        &rules,
    ));
}

/// ITEM-091: apply the tenant's pruning policy, holding out a deterministic
/// share unpruned so the effect is measurable.
///
/// A rewrite that produces nothing (disabled policy, nothing redundant) leaves
/// the payload untouched; a rewrite never fails the request. Held-out requests
/// still record an event, so the dashboard can compare the two groups.
pub(super) fn prune_payload(
    meta: &mut RequestMeta,
    policy: Option<&crate::prune::PrunePolicy>,
    tenant_id: &str,
) {
    let Some(policy) = policy.filter(|policy| policy.enabled) else {
        return;
    };
    // A schema contract is a promise about the answer; do not change its input
    // in the same request that asks it to prove the output (ITEM-091 note).
    if crate::guard::request_schema(&meta.raw_prompt).is_some() {
        return;
    }
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
pub(super) fn with_lint(mut response: Response, lint: Option<&Value>) -> Response {
    let Some(kinds) = lint.and_then(|lint| lint["kinds"].as_array()) else {
        return response;
    };
    let joined = kinds
        .iter()
        .filter_map(|kind| kind.as_str())
        .collect::<Vec<_>>()
        .join(",");
    // A clean scan records an empty list; that is not a finding, so no header.
    if joined.is_empty() {
        return response;
    }
    if let Ok(value) = HeaderValue::from_str(&joined) {
        response.headers_mut().insert("x-inferon-lint", value);
    }
    response
}

/// ITEM-055: tag a response with the request's trace id so a client can attach
/// a quality signal to it out of band.
pub(super) fn with_trace_id(mut response: Response, trace_id: Uuid) -> Response {
    if let Ok(value) = HeaderValue::from_str(&trace_id.to_string()) {
        response.headers_mut().insert("x-inferon-trace-id", value);
    }
    response
}

/// Pipe an upstream byte stream through as text/event-stream, emitting the
/// request trace once the stream is fully consumed. Chunks are yielded
/// immediately (zero-copy; no buffering that would break TTFT) while bytes are
/// accumulated on the side solely for the final usage-frame parse (AC4).
pub(super) fn stream_response(
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
    // The headers outlive the trace: `meta` moves into the stream.

    let header_meta = meta.clone();
    // ITEM-094: when the provider speaks the other surface, the translator wraps
    // the raw stream *before* the telemetry tap — so usage parsing and completion
    // capture read exactly the frames the client receives.
    let stream = match meta.translate_to {
        Some(provider) => futures_util::future::Either::Left(stream_with_trace(
            crate::translate::sse::adapt(
                byte_stream,
                provider,
                meta.model.clone(),
                trace_id.to_string(),
            ),
            started,
            meta,
            tenant_id,
            status,
            tx,
            parse_usage,
        )),
        None => futures_util::future::Either::Right(stream_with_trace(
            byte_stream,
            started,
            meta,
            tenant_id,
            status,
            tx,
            parse_usage,
        )),
    };
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    decorate(response, &header_meta)
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
/// What a guard fallback needs from the routing chain (ITEM-109).
pub(super) struct GuardContext<'a> {
    /// The served step's route — the built-in replay uses it.
    pub(super) route: &'a Route,
    /// The full chain and the step that served, for guard-triggered replays.
    pub(super) chain: &'a [ChainStep],
    pub(super) served: usize,
}

pub(super) async fn guarded_stream_response(
    state: &Arc<AppState>,
    meta: RequestMeta,
    tenant_id: Uuid,
    started: Instant,
    upstream: reqwest::Response,
    context: GuardContext<'_>,
    tx: mpsc::Sender<TraceMessage>,
) -> Response {
    let mut guard_emit = BufferedGuard::new(tx, meta, tenant_id, started);
    let bytes = match upstream.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("guarded upstream body read failed: {e}");
            // 025: a cheap stream that dies before any bytes is the
            // "connection crash" class — fall back, don't surface it.
            return guard_fallback(
                &mut guard_emit,
                state,
                &context,
                GuardReason::UpstreamFailure,
            )
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
            decorate(response, &guard_emit.meta)
        }
        Err(reason) => {
            tracing::warn!(
                "guard rejected a cheap-tier response: {} (reason {}) — replaying",
                tenant_id,
                reason.label()
            );
            guard_fallback(&mut guard_emit, state, &context, reason).await
        }
    }
}

/// One model the guard may replay the original prompt to.
struct ReplayTarget<'a> {
    model: String,
    route: &'a Route,
    /// `Some` when this is a tenant chain step (recorded on the trace);
    /// `None` for the built-in frontier replay.
    step: Option<usize>,
}

/// Where a failed guard may replay, in order (ITEM-109). A tenant opts in by
/// marking a fallback step with the `guard` trigger; without one the pre-109
/// built-in frontier replay stands.
fn replay_targets<'a>(
    original: &serde_json::Value,
    route: &'a Route,
    chain: &'a [ChainStep],
    served: usize,
) -> Vec<ReplayTarget<'a>> {
    let triggers: Vec<&[routing::FallbackTrigger]> =
        chain.iter().map(|step| step.triggers.as_slice()).collect();
    let targets: Vec<ReplayTarget<'a>> =
        routing::all_handling(&triggers, served, routing::FallbackTrigger::Guard)
            .into_iter()
            .map(|index| ReplayTarget {
                model: chain[index].model.clone(),
                route: &chain[index].route,
                step: Some(index),
            })
            .collect();
    if targets.is_empty() {
        return vec![ReplayTarget {
            model: guard::fallback_model(original),
            route,
            step: None,
        }];
    }
    targets
}

struct ReplayFailure {
    model: String,
    detail: String,
    class: ErrorClass,
    message: &'static str,
}

/// ITEM-025/109: transparent fallback after the guard discards a cheap
/// response. Tries every chain step marked with the `guard` trigger in order
/// (or the built-in frontier replay when none is), serving the first valid
/// replacement. Exactly one trace is emitted for the request; when every
/// replay fails the request answers a loud clean 502 (AC4).
pub(super) async fn guard_fallback(
    guard_emit: &mut BufferedGuard,
    state: &AppState,
    context: &GuardContext<'_>,
    reason: GuardReason,
) -> Response {
    let replays = replay_targets(
        &guard_emit.meta.raw_prompt,
        context.route,
        context.chain,
        context.served,
    );
    let mut last_failure: Option<ReplayFailure> = None;

    for replay in replays {
        match replay_once(guard_emit, state, replay, reason).await {
            Ok(response) => return response,
            Err(failure) => last_failure = Some(failure),
        }
    }

    let failure = last_failure.expect("there is always at least one replay target");
    let event = guard::guard_event(
        "fallback_failed",
        reason,
        &guard_emit.meta.model.clone(),
        Some(&failure.detail),
    );
    guard_emit.note_guard(Some(&failure.model), event);
    guard_emit.emit(Usage::default(), false, Outcome::Error, Some(failure.class));
    json_error(
        StatusCode::BAD_GATEWAY,
        "guard_fallback_error",
        failure.message,
    )
}

/// Try one replay target. `Err` means "this target failed, the guard may try
/// the next"; an `Ok` response is terminal (served, or the typed error the
/// request must answer with).
async fn replay_once(
    guard_emit: &mut BufferedGuard,
    state: &AppState,
    replay: ReplayTarget<'_>,
    reason: GuardReason,
) -> Result<Response, ReplayFailure> {
    let guarded_model = guard_emit.meta.model.clone();
    // ITEM-067: the replay is normalized per target — a single DeepSeek
    // deployment cannot accept json_schema at all.
    let downgrade = guard::request_schema(&guard_emit.meta.raw_prompt).is_some()
        && !providers::supports_json_schema(&replay.route.kind, &replay.model);
    let body = match wire_body(
        &guard_emit.meta.raw_prompt,
        replay.route,
        &replay.model,
        downgrade,
    ) {
        Ok(body) => body,
        Err(response) => {
            guard_emit.emit(
                Usage::default(),
                false,
                Outcome::Error,
                Some(ErrorClass::Inferon5xx),
            );
            return Ok(response);
        }
    };
    let attempt = provider_request(&state.http_client, replay.route)
        .body(body)
        .send()
        .await;
    let response = match attempt {
        Err(e) => {
            tracing::error!("guard replay to {} failed: {e}", replay.model);
            return Err(ReplayFailure {
                model: replay.model,
                detail: "transport error".to_string(),
                class: ErrorClass::from_transport(&e),
                message: "frontier fallback request failed",
            });
        }
        Ok(res) if !res.status().is_success() => {
            let detail = format!("http {}", res.status().as_u16());
            tracing::warn!("guard replay to {} rejected: {detail}", replay.model);
            return Err(ReplayFailure {
                model: replay.model,
                detail,
                class: ErrorClass::Guard,
                message: "frontier fallback was rejected",
            });
        }
        Ok(res) => res,
    };

    // Note the event BEFORE buffering the replay so a mid-replay client abort
    // (Drop) still records the guard event (AC3).
    let event = guard::guard_event("fallback", reason, &guarded_model, None);
    guard_emit.note_guard(Some(&replay.model), event);
    if let Some(step) = replay.step {
        guard_emit.meta.routing_event = Some(json!({
            "from_model": guarded_model,
            "to_model": replay.model,
            "reason": "guard",
            "step": step,
        }));
    }
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("guard replay body read failed: {e}");
            guard_emit.emit(
                Usage::default(),
                true,
                Outcome::Truncated,
                Some(ErrorClass::Transport),
            );
            return Ok(json_error(
                StatusCode::BAD_GATEWAY,
                "guard_fallback_error",
                "frontier fallback body read failed",
            ));
        }
    };
    let usage = usage_from_sse_bytes(&bytes).unwrap_or_default();
    // ITEM-067 AC4: never serve an invalid replay silently — record a
    // `fallback_invalid` event when the replay also violates the schema.
    let schema = guard::request_schema(&guard_emit.meta.raw_prompt);
    if guard::check_guarded_sse(&bytes, schema.as_ref()).is_err() {
        guard_emit.note_guard(
            Some(&replay.model),
            guard::guard_event("fallback_invalid", reason, &guarded_model, None),
        );
    }
    guard_emit.emit(usage, false, outcome_for_sse(200, &bytes), None);
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    Ok(decorate(response, &guard_emit.meta))
}

/// Drop-guarded trace emitter for the buffered (non-streamed-to-client)
/// phase: if the client disconnects while the upstream body is still being
/// buffered the handler future is dropped, and only this guard's Drop runs —
/// emitting a truncated trace (ITEM-009 semantics preserved for the guard).
pub(super) struct BufferedGuard {
    pub(super) tx: mpsc::Sender<TraceMessage>,
    pub(super) meta: RequestMeta,
    pub(super) tenant_id: Uuid,
    pub(super) started: Instant,
    pub(super) emitted: Cell<bool>,
    /// ITEM-025: the model that ultimately served (the frontier replay),
    /// overriding `meta.model` on the request's trace — the cost basis.
    pub(super) retarget_model: RefCell<Option<String>>,
    /// ITEM-025 AC3: guard event attached to the single trace once the guard
    /// has fired. None until `note_guard` runs — additive only.
    pub(super) guard_event: RefCell<Option<Value>>,
    /// ITEM-085: the buffered completion, for the dataset pair.
    pub(super) completion: RefCell<Option<crate::capture::Completion>>,
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

    fn emit(
        &self,
        usage: Usage,
        truncated: bool,
        outcome: Outcome,
        error_class: Option<ErrorClass>,
    ) {
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
            self.emit(
                Usage::default(),
                true,
                Outcome::Truncated,
                Some(ErrorClass::ClientAbort),
            );
        }
    }
}

/// One-shot trace emitter for the streaming path. Sends the trace exactly
/// once: on clean/errored completion from the stream loop, or from `Drop` when
/// the client aborts mid-stream — nothing after a dropped future's last poll
/// runs, so `Drop` is the only hook (ITEM-009). `send` is synchronous
/// (`try_send`), so dropping never blocks.
pub(super) struct StreamEmit {
    pub(super) tx: mpsc::Sender<TraceMessage>,
    pub(super) meta: RequestMeta,
    pub(super) tenant_id: Uuid,
    pub(super) status: u16,
    pub(super) started: Instant,
    pub(super) emitted: Cell<bool>,
    pub(super) ttft_ms: Cell<Option<i64>>,
    /// Upstream bytes seen so far, for usage and (ITEM-085) the completion.
    pub(super) raw: RefCell<Vec<u8>>,
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
    fn emit(
        &self,
        usage: Usage,
        truncated: bool,
        outcome: Outcome,
        error_class: Option<ErrorClass>,
    ) {
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
            self.emit(
                Usage::default(),
                true,
                Outcome::Truncated,
                Some(ErrorClass::ClientAbort),
            );
        }
    }
}

/// Tag each upstream chunk with its arrival time (TTFT on the first one), yield
/// it immediately, and once the stream ends queue the trace with token counts
/// parsed from the final usage frame. A client abort mid-stream still emits a
/// truncated trace via StreamEmit's Drop.
pub(super) fn stream_with_trace(
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
