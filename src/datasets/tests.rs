use super::*;
use crate::quality::Outcome;
use serde_json::json;

fn trace(completion: Option<Value>, outcome: Outcome) -> TraceMessage {
    TraceMessage {
        id: uuid::Uuid::new_v4(),
        tenant_id: uuid::Uuid::new_v4(),
        request_model: "deepseek-chat".into(),
        system_prompt_hash: Some("p1".into()),
        prompt_tokens: 10,
        completion_tokens: 4,
        cached_prompt_tokens: None,
        ttft_ms: Some(1),
        total_duration_ms: 10,
        status_code: 200,
        raw_prompt: Some(json!({"messages": [{"role": "user", "content": "hi"}]})),
        raw_completion: completion,
        raw_completion_capped: false,
        waste_detected: None,
        upstream_cost_usd: rust_decimal::Decimal::ZERO,
        cache_savings_usd: rust_decimal::Decimal::ZERO,
        truncated: false,
        guard_event: None,
        routing_event: None,
        router_event: None,
        provider_id: None,
        is_local: false,
        api_key_id: None,
        outcome,
        error_class: None,
        savings_usd: rust_decimal::Decimal::ZERO,
        savings_kind: None,
        cache_affinity: None,
        policy_version: None,
        proxy_ms: Some(1),
        proxy_us: Some(1000),
        example_state: None,
        example_reason: None,
        completion_hash: None,
        session_id: None,
        request_lint: None,
        prune_event: None,
    }
}

fn completion(content: &str) -> Value {
    json!({"choices": [{"message": {"role": "assistant", "content": content}}]})
}

#[test]
fn an_ok_complete_capture_qualifies_with_a_stable_hash() {
    let first = classify(&trace(Some(completion("answer")), Outcome::Ok)).unwrap();
    assert_eq!(first.state, "qualified");
    assert_eq!(first.reason, None);
    assert!(first.completion_hash.is_some());
    let second = classify(&trace(Some(completion("answer")), Outcome::Ok)).unwrap();
    assert_eq!(first.completion_hash, second.completion_hash, "exact dedupe key");
}

#[test]
fn failures_are_excluded_with_why() {
    let mut capped = trace(Some(completion("x")), Outcome::Ok);
    capped.raw_completion_capped = true;
    assert_eq!(classify(&capped).unwrap().reason, Some("capped"));

    let mut truncated = trace(Some(completion("x")), Outcome::Ok);
    truncated.truncated = true;
    assert_eq!(classify(&truncated).unwrap().reason, Some("truncated"));

    let refused = classify(&trace(Some(completion("")), Outcome::Refused)).unwrap();
    assert_eq!(refused.state, "excluded");
    assert_eq!(refused.reason, Some("refused"));

    let mut failed = trace(Some(completion("x")), Outcome::Ok);
    failed.guard_event = Some(json!({"outcome": "fallback_failed"}));
    assert_eq!(classify(&failed).unwrap().reason, Some("guard_failed"));
}

#[test]
fn a_schema_request_must_satisfy_the_schema() {
    let mut valid = trace(Some(completion(r#"{"answer": 42}"#)), Outcome::Ok);
    valid.raw_prompt = Some(json!({
        "messages": [{"role": "user", "content": "hi"}],
        "response_format": {"json_schema": {"schema": {
            "type": "object",
            "properties": {"answer": {"type": "number"}},
            "required": ["answer"]
        }}}
    }));
    assert_eq!(classify(&valid).unwrap().state, "qualified");

    let mut invalid = valid.clone();
    invalid.raw_completion = Some(completion(r#"{"other": 1}"#));
    let excluded = classify(&invalid).unwrap();
    assert_eq!(excluded.state, "excluded");
    assert_eq!(excluded.reason, Some("schema_invalid"));
}

#[test]
fn a_trace_with_no_capture_is_not_an_example_at_all() {
    assert!(classify(&trace(None, Outcome::Error)).is_none());
}
