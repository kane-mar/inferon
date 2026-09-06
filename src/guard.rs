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
/// (`effective_model` = whatever the proxy will actually call upstream, after
/// router/budget overrides.)
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
pub fn validate_sse_container(bytes: &[u8]) -> Result<(), GuardReason> {
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

/// Full flush gate for a buffered guarded response: container integrity
/// (this slice) and — from ITEM-024 — schema conformance of the extracted
/// content against the request's contract. Returns the reason the response
/// must not reach the client.
pub fn check_guarded_sse(bytes: &[u8], _schema: Option<&Value>) -> Result<(), GuardReason> {
    validate_sse_container(bytes)
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
}
