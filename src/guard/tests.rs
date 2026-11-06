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
        assert!(is_cheap_tier("gpt-4o-mini", &[], false));
        assert!(is_cheap_tier("gpt-4o-mini-2024-07-18", &[], false));
        assert!(is_cheap_tier("deepseek-chat", &[], false));
        assert!(is_cheap_tier("deepseek-v4-flash", &[], false));
        assert!(is_cheap_tier("deepseek-v4-flash-vision-exp", &[], false));
        // `deepseek-flash` is the name the API actually returns and the router's
        // DeepSeek downgrade target (ITEM-054) — it must be cheap too.
        assert!(is_cheap_tier("deepseek-flash", &[], false));
        assert!(is_cheap_tier("deepseek-flash-2026-01-01", &[], false));
    }

    #[test]
    fn frontier_models_are_not_cheap_tier() {
        assert!(!is_cheap_tier("gpt-4o", &[], false));
        assert!(!is_cheap_tier("deepseek-v4-pro", &[], false));
        assert!(!is_cheap_tier("claude-3-5-sonnet-20241022", &[], false));
        assert!(!is_cheap_tier("", &[], false));
    }

    #[test]
    fn guarded_requires_schema_and_cheap_model() {
        let with_schema = payload_with_schema();
        assert!(is_guarded_request(&with_schema, "deepseek-chat", &[], false));
        // The router's DeepSeek downgrade target is guarded too (ITEM-054).
        assert!(is_guarded_request(&with_schema, "deepseek-flash", &[], false));
        // Same contract but a frontier model -> not guarded.
        assert!(!is_guarded_request(&with_schema, "deepseek-v4-pro", &[], false));
        // No contract -> never guarded, regardless of model.
        assert!(!is_guarded_request(&plain_payload(), "deepseek-chat", &[], false));
    }

    #[test]
    fn schema_contract_requires_nested_json_schema_object() {
        // response_format present but not json_schema shape (json_object mode).
        let json_mode = serde_json::json!({
            "model": "gpt-4o-mini",
            "response_format": {"type": "json_object"},
            "messages": []
        });
        assert!(!is_guarded_request(&json_mode, "gpt-4o-mini", &[], false));
        // json_schema without a .schema object is not a contract.
        let no_schema = serde_json::json!({
            "model": "gpt-4o-mini",
            "response_format": {"type": "json_schema", "json_schema": {"name": "x"}},
            "messages": []
        });
        assert!(!is_guarded_request(&no_schema, "gpt-4o-mini", &[], false));
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
        assert_eq!(fallback_model(&gpt, &[], false), "gpt-4o");
        let pro = serde_json::json!({"model": "deepseek-v4-pro", "messages": []});
        assert_eq!(fallback_model(&pro, &[], false), "deepseek-v4-pro");
        // Unknown models count as frontier (never downgraded, never guarded).
        let unknown = serde_json::json!({"model": "claude-3-5-sonnet-20241022", "messages": []});
        assert_eq!(fallback_model(&unknown, &[], false), "claude-3-5-sonnet-20241022");
    }

    #[test]
    fn fallback_escalates_cheap_original_requests_to_the_deployment_frontier() {
        // Replaying a cheap original to itself would fail the guard again.
        for cheap in ["gpt-4o-mini", "deepseek-chat", "deepseek-v4-flash", "deepseek-flash"] {
            let p = serde_json::json!({"model": cheap, "messages": []});
            assert_eq!(fallback_model(&p, &[], false), DEFAULT_GUARD_FRONTIER);
        }
        // No model at all -> same escalation.
        assert_eq!(
            fallback_model(&serde_json::json!({ "messages": [] }), &[], false),
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

#[test]
fn a_non_streaming_answer_is_validated_like_a_stream() {
    // ITEM-128: the checker used to run only on streamed answers, so the most
    // common structured request — a plain `json_schema` call — was never checked.
    let schema = Some(serde_json::json!({
        "type": "object",
        "properties": {"answer": {"type": "string"}},
        "required": ["answer"],
        "additionalProperties": false
    }));
    let valid = br#"{"choices":[{"index":0,"message":{"role":"assistant","content":"{\"answer\":\"ok\"}"}}]}"#;
    let invalid =
        br#"{"choices":[{"index":0,"message":{"role":"assistant","content":"{\"wrong\":true}"}}]}"#;
    let not_json =
        br#"{"choices":[{"index":0,"message":{"role":"assistant","content":"not json at all"}}]}"#;
    let empty = br#"{"choices":[{"index":0,"message":{"role":"assistant","content":""}}]}"#;

    assert_eq!(check_guarded_json(valid, schema.as_ref()), Ok(()));
    assert_eq!(
        check_guarded_json(invalid, schema.as_ref()),
        Err(GuardReason::SchemaViolation)
    );
    assert_eq!(
        check_guarded_json(not_json, schema.as_ref()),
        Err(GuardReason::InvalidJsonSyntax)
    );
    assert_eq!(
        check_guarded_json(empty, schema.as_ref()),
        Err(GuardReason::SchemaViolation),
        "no content at all is a violation, never a panic"
    );
}

#[test]
fn a_workspace_treats_its_own_cheap_model_as_cheap() {
    // ITEM-128: the cheap-tier test was a hard-coded model-name list, so a pair
    // the workspace defined itself, and any model it hosts, never got checked.
    let pairs = vec![crate::router::TierPair {
        frontier: "my-frontier".to_string(),
        cheap: "my-small".to_string(),
    }];
    assert!(is_cheap_tier("my-small", &pairs, false));
    assert!(is_cheap_tier("my-small-8b-instruct", &pairs, false));
    assert!(!is_cheap_tier("my-frontier", &pairs, false));
    assert!(is_cheap_tier("any-local-model", &[], true), "local is free");
    // The built-in families still count with no pairs of your own.
    assert!(is_cheap_tier("deepseek-flash", &[], false));
    assert!(!is_cheap_tier("gpt-4o", &[], false));
}

#[test]
fn a_workspace_cheap_model_still_escalates_to_the_deployment_frontier() {
    // ITEM-128: replaying a failing cheap model to itself just fails twice. The
    // escalation decision has to use the same notion of "cheap" as the guard.
    let payload = serde_json::json!({"model": "mock-lie", "messages": []});
    let pairs = vec![crate::router::TierPair {
        frontier: "my-frontier".to_string(),
        cheap: "my-small".to_string(),
    }];
    assert_eq!(
        fallback_model(&payload, &pairs, true),
        DEFAULT_GUARD_FRONTIER,
        "a local model is cheap, so the replay escalates"
    );
    assert_eq!(
        fallback_model(&payload, &pairs, false),
        "mock-lie",
        "an unknown model outside a pair is treated as the tenant's frontier"
    );
    let pair_cheap = serde_json::json!({"model": "my-small-8b", "messages": []});
    assert_eq!(fallback_model(&pair_cheap, &pairs, false), DEFAULT_GUARD_FRONTIER);
}
