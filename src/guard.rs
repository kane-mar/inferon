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
pub fn is_cheap_tier(model: &str) -> bool {
    model.starts_with("gpt-4o-mini")
        || model.starts_with("deepseek-v4-flash")
        || model.starts_with("deepseek-chat")
}

/// Does this request qualify for the guard? True only when BOTH hold:
///   - it carries a JSON-schema contract (`response_format.json_schema.schema`)
///   - the effective model being served is a cheap-tier model
///     (`effective_model` = whatever the proxy will actually call upstream,
///     after router/budget overrides.)
pub fn is_guarded_request(payload: &Value, effective_model: &str) -> bool {
    has_json_schema_contract(payload) && is_cheap_tier(effective_model)
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
        Some(requested) if !is_cheap_tier(requested) => requested.to_string(),
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
pub fn check_guarded_sse(bytes: &[u8], schema: Option<&Value>) -> Result<(), GuardReason> {
    validate_sse_container(bytes)?;
    validate_content(bytes, schema)
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
mod tests {
    use super::*;

    fn payload_with_schema() -> Value {
        serde_json::json!({
            "model": "deepseek-chat",
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "answer",
                    "schema": {"type": "object", "properties": {"answer": {"type": "string"}}}
                }
            },
            "messages": [{"role": "user", "content": "hi"}]
        })
    }

    fn plain_payload() -> Value {
        serde_json::json!({
            "model": "deepseek-chat",
            "messages": [{"role": "user", "content": "hi"}]
        })
    }

    #[test]
    fn cheap_tier_includes_mini_and_deepseek_flash_families() {
        assert!(is_cheap_tier("gpt-4o-mini"));
        assert!(is_cheap_tier("gpt-4o-mini-2024-07-18"));
        assert!(is_cheap_tier("deepseek-chat"));
        assert!(is_cheap_tier("deepseek-v4-flash"));
        assert!(is_cheap_tier("deepseek-v4-flash-vision-exp"));
    }

    #[test]
    fn frontier_models_are_not_cheap_tier() {
        assert!(!is_cheap_tier("gpt-4o"));
        assert!(!is_cheap_tier("deepseek-v4-pro"));
        assert!(!is_cheap_tier("claude-3-5-sonnet-20241022"));
        assert!(!is_cheap_tier(""));
    }

    #[test]
    fn guarded_requires_schema_and_cheap_model() {
        let with_schema = payload_with_schema();
        assert!(is_guarded_request(&with_schema, "deepseek-chat"));
        // Same contract but a frontier model -> not guarded.
        assert!(!is_guarded_request(&with_schema, "deepseek-v4-pro"));
        // No contract -> never guarded, regardless of model.
        assert!(!is_guarded_request(&plain_payload(), "deepseek-chat"));
    }

    #[test]
    fn schema_contract_requires_nested_json_schema_object() {
        // response_format present but not json_schema shape (json_object mode).
        let json_mode = serde_json::json!({
            "model": "gpt-4o-mini",
            "response_format": {"type": "json_object"},
            "messages": []
        });
        assert!(!is_guarded_request(&json_mode, "gpt-4o-mini"));
        // json_schema without a .schema object is not a contract.
        let no_schema = serde_json::json!({
            "model": "gpt-4o-mini",
            "response_format": {"type": "json_schema", "json_schema": {"name": "x"}},
            "messages": []
        });
        assert!(!is_guarded_request(&no_schema, "gpt-4o-mini"));
        assert!(request_schema(&no_schema).is_none());
    }

    #[test]
    fn request_schema_extracts_the_contract() {
        let p = payload_with_schema();
        let schema = request_schema(&p).expect("contract present");
        assert_eq!(schema["type"], "object");
        assert_eq!(request_schema(&plain_payload()), None);
    }

    #[test]
    fn guard_reason_labels_are_stable_and_unique() {
        let labels: Vec<&str> = [
            GuardReason::InvalidJsonSyntax,
            GuardReason::MalformedSchemaContract,
            GuardReason::SchemaViolation,
            GuardReason::CorruptedContainer,
            GuardReason::UpstreamFailure,
        ]
        .iter()
        .map(|r| r.label())
        .collect();
        assert_eq!(labels.len(), 5);
        let mut sorted = labels.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 5, "labels must be unique");
        assert_eq!(labels[0], "invalid_json_syntax");
        assert_eq!(labels[4], "upstream_failure");
    }

    // --- ITEM-023: buffered validation seam (container-level gate) ---

    fn sse_frame(payload: &str) -> String {
        format!("data: {payload}\n\n")
    }

    fn complete_stream() -> String {
        // A realistic chat-completions SSE: deltas, a usage frame, then [DONE].
        format!(
            "{}{}{}",
            sse_frame(r#"{"id":"1","choices":[{"delta":{"content":"{\"a\":1}"}}]}"#),
            sse_frame(r#"{"id":"1","choices":[],"usage":{"prompt_tokens":5}}"#),
            "data: [DONE]\n\n"
        )
    }

    #[test]
    fn complete_well_formed_stream_passes_the_container_gate() {
        assert_eq!(validate_sse_container(complete_stream().as_bytes()), Ok(()));
    }

    #[test]
    fn empty_stream_is_an_upstream_failure() {
        assert_eq!(
            validate_sse_container(b""),
            Err(GuardReason::UpstreamFailure)
        );
    }

    #[test]
    fn frames_without_done_are_a_truncated_upstream() {
        let truncated = format!("{}{}", sse_frame(r#"{"a":1}"#), sse_frame(r#"{"b":2}"#));
        assert_eq!(
            validate_sse_container(truncated.as_bytes()),
            Err(GuardReason::UpstreamFailure)
        );
    }

    #[test]
    fn garbage_without_json_frames_is_a_corrupted_container() {
        assert_eq!(
            validate_sse_container(b"data: not-json\n\ndata: still-not-json\n\n"),
            Err(GuardReason::CorruptedContainer)
        );
        assert_eq!(
            validate_sse_container(b"random bytes, no data prefix at all"),
            Err(GuardReason::CorruptedContainer)
        );
    }

    #[test]
    fn non_json_data_lines_do_not_poison_a_valid_stream() {
        // Keep-alive comments / blank lines are legal SSE; only the JSON
        // frames and [DONE] decide the verdict.
        let stream = format!("\n{}", complete_stream());
        assert_eq!(validate_sse_container(stream.as_bytes()), Ok(()));
    }

    #[test]
    fn carriage_returns_are_tolerated() {
        let crlf = complete_stream().replace("\n\n", "\r\n\r\n");
        assert_eq!(validate_sse_container(crlf.as_bytes()), Ok(()));
    }

    // --- ITEM-024: schema compile + validate ---

    fn answer_schema() -> Value {
        serde_json::json!({"type": "object", "properties": {"answer": {"type": "string"}}, "required": ["answer"]})
    }

    /// SSE stream whose delta content is `payload` (a JSON string inside a
    /// JSON string — double-encoded like a real chat completion).
    fn stream_with_content(content_json: &str) -> String {
        let escaped = content_json.replace('\\', "\\\\").replace('"', "\\\"");
        let frame =
            sse_frame(&format!(r#"{{"id":"1","choices":[{{"delta":{{"content":"{escaped}"}}}}]}}"#));
        format!("{frame}data: [DONE]\n\n")
    }

    #[test]
    fn extract_content_concatenates_deltas_in_order() {
        // Content split across two delta frames must rejoin in order.
        let stream = format!(
            "{}{}{}",
            sse_frame(r#"{"id":"1","choices":[{"delta":{"content":"{\"an"}}]}"#),
            sse_frame(r#"{"id":"1","choices":[{"delta":{"content":"swer\":\"42\"}"}}]}"#),
            "data: [DONE]\n\n"
        );
        assert_eq!(
            extract_content(stream.as_bytes()).as_deref(),
            Some("{\"answer\":\"42\"}")
        );
    }

    #[test]
    fn extract_content_skips_non_delta_frames() {
        // Usage frames, role frames and [DONE] contribute nothing.
        let stream = format!(
            "{}{}{}",
            sse_frame(r#"{"id":"1","choices":[{"delta":{"role":"assistant"}}]}"#),
            sse_frame(r#"{"id":"1","choices":[{"delta":{"content":"hi"}}]}"#),
            sse_frame(r#"{"id":"1","choices":[],"usage":{"prompt_tokens":5}}"#),
        ) + "data: [DONE]\n\n";
        assert_eq!(extract_content(stream.as_bytes()).as_deref(), Some("hi"));
    }

    #[test]
    fn extract_content_is_none_when_no_assistant_content() {
        let stream = format!("{}{}", sse_frame(r#"{"id":"1","choices":[]}"#), "data: [DONE]\n\n");
        assert_eq!(extract_content(stream.as_bytes()), None);
    }

    #[test]
    fn valid_json_matching_schema_passes() {
        let stream = stream_with_content(r#"{"answer":"42"}"#);
        assert_eq!(
            validate_content(stream.as_bytes(), Some(&answer_schema())),
            Ok(())
        );
    }

    #[test]
    fn invalid_json_syntax_is_classified() {
        // Model emitted `{not valid json` — syntax failure, not schema.
        let stream = stream_with_content("{not valid json");
        assert_eq!(
            validate_content(stream.as_bytes(), Some(&answer_schema())),
            Err(GuardReason::InvalidJsonSyntax)
        );
    }

    #[test]
    fn well_formed_json_violating_schema_is_classified() {
        // Valid JSON, missing the required "answer" property.
        let stream = stream_with_content(r#"{"other":1}"#);
        assert_eq!(
            validate_content(stream.as_bytes(), Some(&answer_schema())),
            Err(GuardReason::SchemaViolation)
        );
    }

    #[test]
    fn malformed_schema_contract_is_classified() {
        // The request's own schema is not a valid JSON Schema.
        let bad_schema = serde_json::json!({ "type": 42 });
        let stream = stream_with_content(r#"{"answer":"42"}"#);
        assert_eq!(
            validate_content(stream.as_bytes(), Some(&bad_schema)),
            Err(GuardReason::MalformedSchemaContract)
        );
    }

    #[test]
    fn missing_content_is_a_defined_schema_violation_not_a_panic() {
        // Complete stream, no content to validate (ITEM-024 AC3 edge case).
        let stream = format!("{}{}", sse_frame(r#"{"id":"1","choices":[]}"#), "data: [DONE]\n\n");
        assert_eq!(
            validate_content(stream.as_bytes(), Some(&answer_schema())),
            Err(GuardReason::SchemaViolation)
        );
    }

    #[test]
    fn valid_stream_without_a_contract_passes_content_check() {
        // Guard can't enforce what wasn't asked for (no schema).
        let stream = stream_with_content("anything at all");
        assert_eq!(validate_content(stream.as_bytes(), None), Ok(()));
    }

    #[test]
    fn check_guarded_sse_runs_container_then_content_gate() {
        let good = stream_with_content(r#"{"answer":"42"}"#);
        assert_eq!(
            check_guarded_sse(good.as_bytes(), Some(&answer_schema())),
            Ok(())
        );
        // Schema violation inside a well-formed container -> SchemaViolation.
        let bad = stream_with_content(r#"{"other":1}"#);
        assert_eq!(
            check_guarded_sse(bad.as_bytes(), Some(&answer_schema())),
            Err(GuardReason::SchemaViolation)
        );
        // Truncated container fails before content even runs.
        assert_eq!(
            check_guarded_sse(b"data: {\"a\":1}\n\n", Some(&answer_schema())),
            Err(GuardReason::UpstreamFailure)
        );
    }

    // --- ITEM-025: transparent fallback circuit helpers ---

    #[test]
    fn fallback_replays_the_originally_requested_frontier_model() {
        // Router downgraded a gpt-4o request to gpt-4o-mini; the tenant's own
        // frontier (pre-downgrade) is the replay target.
        let gpt = serde_json::json!({"model": "gpt-4o", "messages": []});
        assert_eq!(fallback_model(&gpt), "gpt-4o");
        let pro = serde_json::json!({"model": "deepseek-v4-pro", "messages": []});
        assert_eq!(fallback_model(&pro), "deepseek-v4-pro");
        // Unknown models count as frontier (never downgraded, never guarded).
        let unknown = serde_json::json!({"model": "claude-3-5-sonnet-20241022", "messages": []});
        assert_eq!(fallback_model(&unknown), "claude-3-5-sonnet-20241022");
    }

    #[test]
    fn fallback_escalates_cheap_original_requests_to_the_deployment_frontier() {
        // Replaying a cheap original to itself would fail the guard again.
        for cheap in ["gpt-4o-mini", "deepseek-chat", "deepseek-v4-flash"] {
            let p = serde_json::json!({"model": cheap, "messages": []});
            assert_eq!(fallback_model(&p), DEFAULT_GUARD_FRONTIER);
        }
        // No model at all -> same escalation.
        assert_eq!(
            fallback_model(&serde_json::json!({ "messages": [] })),
            DEFAULT_GUARD_FRONTIER
        );
    }

    #[test]
    fn repeated_schema_validation_is_stable_across_the_cache() {
        // ITEM-027 AC4: repeated guarded requests share one compiled validator.
        // Semantics must be identical whether the schema was cache-hit or
        // compiled fresh (validate passes for conforming, rejects violating).
        let good = stream_with_content(r#"{"answer":"42"}"#);
        let bad = stream_with_content(r#"{"other":1}"#);
        for _ in 0..3 {
            assert_eq!(
                validate_content(good.as_bytes(), Some(&answer_schema())),
                Ok(())
            );
            assert_eq!(
                validate_content(bad.as_bytes(), Some(&answer_schema())),
                Err(GuardReason::SchemaViolation)
            );
        }
    }

    #[test]
    fn guard_event_shape_is_stable() {
        let ok = guard_event("fallback", GuardReason::SchemaViolation, "gpt-4o-mini", None);
        assert_eq!(ok["outcome"], "fallback");
        assert_eq!(ok["reason"], "schema_violation");
        assert_eq!(ok["guarded_model"], "gpt-4o-mini");
        assert!(ok.get("detail").is_none());
        let failed = guard_event(
            "fallback_failed",
            GuardReason::UpstreamFailure,
            "deepseek-chat",
            Some("http 500"),
        );
        assert_eq!(failed["outcome"], "fallback_failed");
        assert_eq!(failed["detail"], "http 500");
    }
}
