// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;

/// Request-derived fields shared by every trace of this request.
#[derive(Clone)]
pub(super) struct RequestMeta {
    pub(super) id: Uuid,
    pub(super) model: String,
    pub(super) system_prompt_hash: Option<String>,
    pub(super) raw_prompt: Value,
    /// ITEM-127: the body exactly as the client sent it. Forwarded unchanged when
    /// routing rewrites nothing, so a large prompt is parsed once instead of being
    /// cloned and re-serialised before it is even sent. `Bytes` clones are free.
    pub(super) received: Option<axum::body::Bytes>,
    /// ITEM-045: set when a fallback chain changed the serving model.
    pub(super) routing_event: Option<Value>,
    /// ITEM-054: set when the heuristic router downshifted the model.
    pub(super) router_event: Option<Value>,
    /// ITEM-063: set when cache affinity changed the serving model.
    pub(super) cache_affinity: Option<Value>,
    /// ITEM-064: the adaptive policy version in force for this request.
    pub(super) policy_version: Option<i64>,
    /// ITEM-066/092/135: the proxy's own pre-upstream time, in ms for the
    /// dashboard and µs for the performance suite. Measured from the moment the
    /// request body is complete — the time a client spends uploading its prompt
    /// is `body_ms`, not gateway work.
    pub(super) proxy_ms: Option<i64>,
    pub(super) proxy_us: Option<i64>,
    /// ITEM-135: request start → body complete. The client's upload, kept so a
    /// slow sender is visible without being mistaken for proxy latency.
    pub(super) body_ms: Option<i64>,
    /// ITEM-140: money a protection stopped on this request (see
    /// `cost::projected_cost`). Set at the decision site, never recomputed later.
    pub(super) prevented: Option<(rust_decimal::Decimal, &'static str)>,
    /// ITEM-067: set when the wire format was downgraded (json_schema →
    /// json_object) because the target cannot honour a schema contract.
    pub(super) wire_format: Option<&'static str>,
    /// ITEM-047: provider + API key attribution for the trace.
    pub(super) provider_id: Option<Uuid>,
    pub(super) api_key_id: Option<Uuid>,
    /// ITEM-077: served by a locally hosted provider — costed at zero.
    pub(super) is_local: bool,
    /// ITEM-088: whether raw payloads may be stored for this tenant.
    pub(super) capture_payloads: bool,
    /// ITEM-093: the client's transaction id, when it supplied a valid one.
    pub(super) session_id: Option<String>,
    /// ITEM-090: pre-flight payload findings (None = nothing fired).
    pub(super) lint: Option<Value>,
    /// ITEM-091: what pruning removed (or that this request was held out).
    pub(super) prune_event: Option<Value>,
    /// ITEM-094: the surface the provider spoke, when it was not the client's.
    pub(super) translate_to: Option<Surface>,
    /// ITEM-095: this request's velocity window, for the response headers.
    pub(super) rate_limit: Option<(crate::ratelimit::Limits, crate::ratelimit::Usage)>,
    /// ITEM-096: detection and the two redaction modes for this request.
    pub(super) pii: PiiPlan,
    /// ITEM-100: why the provider was chosen (`only`, `weight`, `round_robin`).
    pub(super) provider_selection: Option<&'static str>,
    /// Whether the client asked for a stream. ITEM-131 reads it to decide
    /// whether an attempt that has said nothing may still be abandoned.
    pub(super) is_stream: bool,
}

/// The client's own `stream` flag, read once from the payload.
fn payload_stream(payload: &serde_json::Value) -> bool {
    payload["stream"].as_bool().unwrap_or(false)
}

impl RequestMeta {
    /// The payload's size, without re-serialising it (ITEM-127). Estimated token
    /// counts are built from this, and a 300k-token prompt is a megabyte.
    pub(super) fn payload_bytes(&self) -> usize {
        self.received
            .as_ref()
            .map_or_else(|| self.raw_prompt.to_string().len(), |bytes| bytes.len())
    }

    /// The request-derived fields every trace shares. `system_prompt_hash` is
    /// passed in because the two ingress surfaces hash different things.
    pub(super) fn new(
        payload: Value,
        tenant: &crate::Tenant,
        session_id: Option<String>,
        system_prompt_hash: Option<String>,
    ) -> Self {
        let is_stream = payload_stream(&payload);
        Self {
            id: Uuid::new_v4(),
            model: payload["model"].as_str().unwrap_or("unknown").to_string(),
            system_prompt_hash,
            raw_prompt: payload,
            received: None,
            routing_event: None,
            router_event: None,
            cache_affinity: None,
            policy_version: None,
            proxy_ms: None,
            proxy_us: None,
            body_ms: None,
            prevented: None,
            wire_format: None,
            provider_id: None,
            api_key_id: tenant.api_key_id,
            is_local: false,
            capture_payloads: true,
            session_id,
            lint: None,
            prune_event: None,
            translate_to: None,
            rate_limit: None,
            pii: PiiPlan::default(),
            provider_selection: None,
            is_stream,
        }
    }
}

/// ITEM-096: what the trace path should do with this payload.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PiiPlan {
    pub(crate) detect: bool,
    pub(crate) redact_storage: bool,
    pub(crate) redact_wire: bool,
}

impl PiiPlan {
    /// Whether the wire payload was left unredacted on purpose (the holdout).
    pub(crate) fn wire_held_out(&self, tenant_id: Uuid, prompt_hash: Option<&str>) -> bool {
        let seed = format!("{tenant_id}:{}", prompt_hash.unwrap_or_default());
        let policy = crate::prune::PrunePolicy {
            holdout_percent: crate::pii::WIRE_HOLDOUT,
            ..Default::default()
        };
        crate::prune::in_holdout(&seed, &policy)
    }
}

/// ITEM-096: scan (and optionally redact) a payload for storage.
///
/// Returns the payload to store and the trace record. The record carries counts
/// by class only — never a matched value — so the trace cannot leak what it
/// found. Redaction here is what the dataset export and the audit share read.
/// ITEM-096: redact the payload that leaves the perimeter, when the tenant
/// opted in. Held-out requests are forwarded as written so the effect on quality
/// is measurable — the same discipline as ITEM-091.
pub(super) fn redact_wire(meta: &mut RequestMeta, tenant_id: Uuid) {
    if !meta.pii.redact_wire
        || meta
            .pii
            .wire_held_out(tenant_id, meta.system_prompt_hash.as_deref())
    {
        return;
    }
    let (redacted, _) = crate::pii::redact_value(&meta.raw_prompt);
    meta.raw_prompt = redacted;
}

/// ITEM-096: scan (and optionally redact) one payload for storage.
///
/// The counts are by class only — never a matched value — so the trace cannot
/// leak what it found. Redaction here is what the dataset export and the audit
/// share read, which is why it happens before the payload is stored.
pub(super) fn pii_payload(plan: PiiPlan, payload: Option<Value>) -> (Option<Value>, crate::pii::Counts) {
    let Some(payload) = payload else {
        return (None, crate::pii::Counts::default());
    };
    if !plan.detect {
        return (Some(payload), crate::pii::Counts::default());
    }
    if plan.redact_storage {
        let (redacted, counts) = crate::pii::redact_value(&payload);
        return (Some(redacted), counts);
    }
    let counts = crate::pii::scan_value(&payload);
    (Some(payload), counts)
}

/// The outcome of a proxied request, as recorded on its trace.
pub(super) struct TraceOutcome {
    pub(super) status: u16,
    pub(super) total_duration_ms: i64,
    pub(super) usage: Usage,
    pub(super) ttft_ms: Option<i64>,
    pub(super) raw_completion: Option<Value>,
    /// ITEM-085: the capture hit the size cap (a head, not a full pair).
    pub(super) raw_completion_capped: bool,
    pub(super) truncated: bool,
    /// ITEM-055: whether the response was usable.
    pub(super) outcome: Outcome,
    /// ITEM-057: why a failure/abort happened (`None` on success).
    pub(super) error_class: Option<ErrorClass>,
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

pub(super) fn build_trace(meta: &RequestMeta, tenant_id: Uuid, outcome: TraceOutcome) -> TraceMessage {
    // ITEM-096: the stored payload is the redacted one; the trace keeps counts.
    let (raw_prompt, prompt_counts) = pii_payload(
        meta.pii,
        meta.capture_payloads.then(|| meta.raw_prompt.clone()),
    );
    let (raw_completion, completion_counts) = pii_payload(
        meta.pii,
        meta.capture_payloads.then_some(outcome.raw_completion).flatten(),
    );
    let mut counts = prompt_counts;
    counts.merge(&completion_counts);
    let pii = (meta.pii.detect && (meta.capture_payloads || meta.pii.redact_wire)).then(|| {
        let storage = if meta.pii.redact_storage { "redacted" } else { "stored" };
        let wire = if !meta.pii.redact_wire {
            "off"
        } else if meta.pii.wire_held_out(tenant_id, meta.system_prompt_hash.as_deref()) {
            "held_out"
        } else {
            "redacted"
        };
        crate::pii::trace_record(&counts, storage, wire)
    });
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
        raw_prompt,
        raw_completion,
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
        body_ms: meta.body_ms,
        prevented_usd: meta.prevented.map(|(usd, _)| usd).unwrap_or_default(),
        prevented_kind: meta.prevented.map(|(_, kind)| kind),
        example_state: None,
        example_reason: None,
        completion_hash: None,
        session_id: meta.session_id.clone(),
        request_lint: meta.lint.clone(),
        prune_event: meta.prune_event.clone(),
        pii,
        provider_selection: meta.provider_selection,
        is_local: meta.is_local,
    }
}

/// Refuse a model the tenant disabled with a typed, traced 400 (ITEM-044 AC5).
pub(super) fn ensure_model_enabled(
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
pub(super) fn trace_error(
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
pub(super) fn send_trace(tx: &mpsc::Sender<TraceMessage>, msg: TraceMessage) {
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

