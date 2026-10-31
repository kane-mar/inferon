use super::*;

#[test]
fn the_wire_body_pins_the_eval_model_and_turns_streaming_off() {
    let pinned = serde_json::json!({
        "model": "gpt-4o",
        "stream": true,
        "messages": [{"role": "user", "content": "hi"}]
    });
    let body: Value = serde_json::from_slice(&budget_body(&pinned, "deepseek-flash").unwrap()).unwrap();
    assert_eq!(body["model"], "deepseek-flash");
    assert_eq!(body["stream"], false);
    assert_eq!(body["messages"][0]["content"], "hi", "the pinned input is untouched");
}

#[test]
fn an_unsupported_pinned_request_is_a_recorded_failure_not_a_zero() {
    // A case that cannot even be sent is `failed`, never scored 0.
    let result = failed_case(uuid::Uuid::nil(), "request_unsupported", "why".to_string());
    assert_eq!(result.status, "request_unsupported");
    assert_eq!(result.score, None);
    assert!(result.detail.unwrap().contains("why"));
}
