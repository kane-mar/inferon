//! Schema guard (Slice 4, ITEM-023..027).
//!
//! Guarantees that cheap-tier responses honour the caller's JSON-schema
//! contract: guarded streams are buffered, validated against the request's
//! own `response_format.json_schema.schema`, and only flushed on success; a
//! failed cheap response is discarded and transparently replayed to a frontier
//! model. Pure-passthrough traffic is byte-for-byte unchanged.
//!
//! Module split (slices in pull order):
//!   - ITEM-023 (this file, guard classification + buffered seam types)
//!   - ITEM-024 schema compile + validate (`GuardReason` classification)
//!   - ITEM-025 transparent fallback circuit
//!   - ITEM-026 guard telemetry (trace fields + views)
//!   - ITEM-027 activation defaults + opt-out + smoke scenario

use serde_json::Value;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

/// Compiled-schema cache (ITEM-027 AC4): guarded requests repeat the same
/// `response_format` schema contract, and compiling it with jsonschema on
/// every flush gate dominates the <5ms hot-path budget on the durable host.
/// Bounded LRU-ish cache keyed by the schema text (serde_json maps sort keys,
/// so the same contract always yields the same key). Validator owns its
/// compiled state (no borrow), so entries are shareable behind an Arc.
const VALIDATOR_CACHE_CAPACITY: usize = 64;

type ValidatorCache = Mutex<VecDeque<(String, Arc<jsonschema::Validator>)>>;

fn validator_cache() -> &'static ValidatorCache {
    static CACHE: OnceLock<ValidatorCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(VecDeque::with_capacity(VALIDATOR_CACHE_CAPACITY)))
}

/// Compile (or fetch from the cache) a validator for `schema`. Compilation
/// failures are the request-side `MalformedSchemaContract` class.
fn compiled_validator(schema: &Value) -> Result<Arc<jsonschema::Validator>, GuardReason> {
    let key = schema.to_string();
    let mut cache = validator_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(pos) = cache.iter().position(|(k, _)| k == &key) {
        let (key, validator) = cache.remove(pos).expect("position found above");
        cache.push_back((key, Arc::clone(&validator))); // LRU touch
        return Ok(validator);
    }
    let validator = Arc::new(
        jsonschema::validator_for(schema)
            .map_err(|_| GuardReason::MalformedSchemaContract)?,
    );
    if cache.len() >= VALIDATOR_CACHE_CAPACITY {
        cache.pop_front();
    }
    cache.push_back((key, Arc::clone(&validator)));
    Ok(validator)
}

/// Guard failure reasons — verbatim from the spec's `fallback_circuit.rs`
/// list (ITEM-024 AC2). Kept here so every slice shares one vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardReason {
    /// Invalid JSON syntax emitted by model.
    InvalidJsonSyntax,
    /// Malformed JSON schema contract (request-side).
    MalformedSchemaContract,
    /// Schema validation failed (well-formed JSON, wrong shape).
    SchemaViolation,
    /// Corrupted non-JSON container emitted by engine.
    CorruptedContainer,
    /// Guarded-endpoint 5xx or connection crash.
    UpstreamFailure,
}

impl GuardReason {
    pub fn label(self) -> &'static str {
        match self {
            GuardReason::InvalidJsonSyntax => "invalid_json_syntax",
            GuardReason::MalformedSchemaContract => "malformed_schema_contract",
            GuardReason::SchemaViolation => "schema_violation",
            GuardReason::CorruptedContainer => "corrupted_container",
            GuardReason::UpstreamFailure => "upstream_failure",
        }
    }
}

/// Models that count as the cheap tier (the guard's reason to exist).
/// Cheap tier = the router's downgrade target family + the DeepSeek flash
/// family that live traffic already uses (deepseek-chat resolves to
/// v4-flash). Anything else (gpt-4o, deepseek-v4-pro, unknown) is frontier.
pub fn is_cheap_tier(model: &str, tenant_pairs: &[crate::router::TierPair], is_local: bool) -> bool {
    // ITEM-128: "cheap" is a property of this workspace, not a hard-coded list of
    // model names. A model on your own hardware is the cheapest one there is, and
    // the cheap side of a pair you defined is exactly what the router downshifts to.
    is_local
        || BUILTIN_CHEAP_TIERS
            .iter()
            .any(|prefix| model.starts_with(prefix))
        || crate::router::TIER_PAIRS
            .iter()
            .any(|(_, cheap)| model.starts_with(cheap))
        || tenant_pairs
            .iter()
            .any(|pair| model.starts_with(&pair.cheap) && !model.starts_with(&pair.frontier))
}

/// The model families that are cheap on every deployment.
const BUILTIN_CHEAP_TIERS: [&str; 4] = [
    "gpt-4o-mini",
    "deepseek-chat",
    "deepseek-flash",
    "deepseek-v4-flash",
];

/// Does this request qualify for the guard? True only when BOTH hold:
///   - it carries a JSON-schema contract (`response_format.json_schema.schema`)
///   - the effective model being served is a cheap-tier model
///     (`effective_model` = whatever the proxy will actually call upstream,
///     after router/budget overrides.)
pub fn is_guarded_request(
    payload: &Value,
    effective_model: &str,
    tenant_pairs: &[crate::router::TierPair],
    is_local: bool,
) -> bool {
    has_json_schema_contract(payload) && is_cheap_tier(effective_model, tenant_pairs, is_local)
}

/// The request-side schema contract, when present and object-shaped.
/// Source of truth: `response_format.json_schema.schema` (OpenAI structured
/// outputs — the existing client contract, no new surface).
pub fn request_schema(payload: &Value) -> Option<Value> {
    payload["response_format"]["json_schema"]["schema"]
        .as_object()
        .cloned()
        .map(Value::Object)
}

fn has_json_schema_contract(payload: &Value) -> bool {
    request_schema(payload).is_some()
}

/// Frontier replay target when a guarded request named a cheap model of its
/// own (replaying to the same failing model would fail the guard again): the
/// strongest model the deployment's default upstream serves (ITEM-025).
pub const DEFAULT_GUARD_FRONTIER: &str = "deepseek-v4-pro";

/// ITEM-025 AC1: the tenant's frontier model for the guard replay. Defaults to
/// the model the tenant originally requested — pre-router/budget downgrade,
/// i.e. their own frontier in a single-upstream world. Escalates to
/// [`DEFAULT_GUARD_FRONTIER`] when the original request itself named a
/// cheap-tier model. Pure function of the original (pre-override) payload.
pub fn fallback_model(original_payload: &Value) -> String {
    match original_payload["model"].as_str() {
        Some(requested) if !is_cheap_tier(requested, &[], false) => requested.to_string(),
        _ => DEFAULT_GUARD_FRONTIER.to_string(),
    }
}

/// ITEM-025 AC3: JSON guard event attached to the request's single trace.
/// Shape is the ITEM-026 contract for views and counts:
///   outcome: "fallback" (replay delivered) | "fallback_failed" (AC4)
///   reason: GuardReason label that rejected the cheap response
///   guarded_model: the cheap model the guard fired on (the trace's
///     request_model is retargeted to the model that ultimately served)
///   detail: present on fallback_failed — the replay's HTTP status or
///     "transport error"
pub fn guard_event(
    outcome: &str,
    reason: GuardReason,
    guarded_model: &str,
    detail: Option<&str>,
) -> Value {
    let mut event = serde_json::json!({
        "outcome": outcome,
        "reason": reason.label(),
        "guarded_model": guarded_model,
    });
    if let Some(detail) = detail {
        event["detail"] = serde_json::Value::String(detail.to_string());
    }
    event
}

/// ITEM-026 AC1: guard event for a guarded request whose cheap response
/// passed the flush gate and was served — outcome "flushed", no failure
/// reason. Every guard outcome is recorded; this is the pass case.
pub fn guard_flushed_event(guarded_model: &str) -> Value {
    serde_json::json!({
        "outcome": "flushed",
        "guarded_model": guarded_model,
    })
}

/// Validate the *container* of a buffered guarded SSE stream (ITEM-023 scope).
/// Returns the concatenated raw SSE bytes only when the stream is a complete,
/// well-formed chat-completions event stream: at least one JSON `data:` frame
/// followed by `[DONE]`. Anything else is a corrupted/non-JSON container
/// (`CorruptedContainer`) or a truncated stream (`UpstreamFailure` when the
/// upstream died before [DONE]).
///
/// Schema-level validation of the extracted content is ITEM-024's slice —
/// this gate exists so the buffered flush decision has a real, testable seam
/// before the schema machinery lands.
pub(crate) fn validate_sse_container(bytes: &[u8]) -> Result<(), GuardReason> {
    if bytes.is_empty() {
        return Err(GuardReason::UpstreamFailure);
    }
    let mut saw_frame = false;
    let mut saw_done = false;
    for line in bytes.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(data) = line.strip_prefix(b"data:") else {
            continue;
        };
        let data = trim_ws(data);
        if data == b"[DONE]" {
            saw_done = true;
            continue;
        }
        if serde_json::from_slice::<Value>(data).is_ok() {
            saw_frame = true;
        }
    }
    if saw_frame && saw_done {
        Ok(())
    } else if saw_frame {
        // Frames arrived but the upstream never finished cleanly.
        Err(GuardReason::UpstreamFailure)
    } else {
        // Nothing parseable at all — corrupted/non-JSON container.
        Err(GuardReason::CorruptedContainer)
    }
}

/// Full flush gate for a buffered guarded response (ITEM-023 + ITEM-024):
/// container integrity first, then schema conformance of the extracted
/// content against the request's contract. Returns the reason the response
/// must not reach the client.
/// ITEM-086: does captured assistant content satisfy `schema`? Used by the
/// dataset qualification pass, which must verify pairs it did not guard.
pub fn content_matches_schema(content: &str, schema: &Value) -> bool {
    let Ok(instance) = serde_json::from_str::<Value>(content) else {
        return false;
    };
    compiled_validator(schema).is_ok_and(|validator| validator.validate(&instance).is_ok())
}

pub fn check_guarded_sse(bytes: &[u8], schema: Option<&Value>) -> Result<(), GuardReason> {
    validate_sse_container(bytes)?;
    validate_content(bytes, schema)
}

/// ITEM-128: the same check for a buffered (non-streaming) answer. The contract
/// lives on `choices[0].message.content`, exactly as it does for a stream's
/// assembled deltas — a client that does not stream is not exempt from the
/// promise it asked us to enforce.
pub fn check_guarded_json(bytes: &[u8], schema: Option<&Value>) -> Result<(), GuardReason> {
    let Ok(body) = serde_json::from_slice::<Value>(bytes) else {
        return Err(GuardReason::InvalidJsonSyntax);
    };
    let content = body["choices"]
        .as_array()
        .and_then(|choices| choices.first())
        .and_then(|choice| choice["message"]["content"].as_str());
    let Some(content) = content else {
        return Err(GuardReason::SchemaViolation);
    };
    validate_json_content(content, schema)
}

/// The schema check itself, shared by both shapes once the answer's text is known.
fn validate_json_content(content: &str, schema: Option<&Value>) -> Result<(), GuardReason> {
    let Some(schema) = schema else {
        return Ok(());
    };
    if content.trim().is_empty() {
        return Err(GuardReason::SchemaViolation);
    }
    let instance: Value =
        serde_json::from_str(content).map_err(|_| GuardReason::InvalidJsonSyntax)?;
    compiled_validator(schema)?
        .validate(&instance)
        .map_err(|_| GuardReason::SchemaViolation)
}

/// Concatenated plain-text content of the response, per the spec: content
/// under validation is `choices[0].message.content`. Streaming deltas arrive
/// as successive `choices[0].delta.content` strings across frames; the
/// assistant message (role present, non-empty delta) is assembled in order.
/// Returns `None` when the stream carries no assistant content at all.
pub(crate) fn extract_content(bytes: &[u8]) -> Option<String> {
    let mut content = String::new();
    for line in bytes.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(data) = line.strip_prefix(b"data:") else {
            continue;
        };
        let data = trim_ws(data);
        if data == b"[DONE]" {
            continue;
        }
        let Ok(frame) = serde_json::from_slice::<Value>(data) else {
            continue;
        };
        let choice = frame["choices"].as_array().and_then(|a| a.first());
        let delta = choice.and_then(|c| c["delta"]["content"].as_str());
        if let Some(delta) = delta {
            content.push_str(delta);
        }
    }
    (!content.is_empty()).then_some(content)
}

/// ITEM-024: validate the accumulated guarded response against the request's
/// JSON-schema contract. Failure classes per spec `fallback_circuit.rs`:
///   - content is not valid JSON syntax  -> InvalidJsonSyntax
///   - the request's schema does not compile -> MalformedSchemaContract
///   - well-formed JSON violates the schema -> SchemaViolation
///
/// Empty/missing content on an otherwise complete stream is a defined edge
/// case (no content to validate -> schema violation, never a panic).
pub(crate) fn validate_content(bytes: &[u8], schema: Option<&Value>) -> Result<(), GuardReason> {
    let Some(schema) = schema else {
        // No contract to enforce — the guard has nothing to check. (Guarded
        // requests always carry one; this keeps the function total.)
        return Ok(());
    };
    let Some(content) = extract_content(bytes) else {
        // Complete stream but no assistant content to validate — defined edge
        // case (ITEM-024 AC3), classified as a schema violation, never a panic.
        return Err(GuardReason::SchemaViolation);
    };
    let instance: Value =
        serde_json::from_str(&content).map_err(|_| GuardReason::InvalidJsonSyntax)?;
    let validator = compiled_validator(schema)?;
    validator
        .validate(&instance)
        .map_err(|_| GuardReason::SchemaViolation)
}

fn trim_ws(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

#[cfg(test)]
mod tests;
