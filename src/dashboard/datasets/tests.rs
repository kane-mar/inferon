use super::*;
use serde_json::json;

#[test]
fn a_line_is_the_prompt_messages_plus_the_assistant_turn() {
    let prompt = json!({"messages": [
        {"role": "system", "content": "be terse"},
        {"role": "user", "content": "extract"}
    ]});
    let completion = json!({"choices": [{"message": {"role": "assistant", "content": "{\"a\":1}"}}]});

    let line = jsonl_line(&prompt, &completion, "deepseek-chat", Uuid::nil()).unwrap();
    let parsed: Value = serde_json::from_str(&line).unwrap();

    assert_eq!(parsed["messages"].as_array().unwrap().len(), 3);
    assert_eq!(parsed["messages"][2]["role"], "assistant");
    assert_eq!(parsed["messages"][2]["content"], "{\"a\":1}");
    assert_eq!(parsed["model"], "deepseek-chat");
    assert_eq!(parsed["trace_id"], Uuid::nil().to_string());
}

#[test]
fn a_line_without_a_prompt_or_a_message_is_skipped() {
    let completion = json!({"choices": [{"message": {"content": "x"}}]});
    assert!(jsonl_line(&json!({}), &completion, "m", Uuid::nil()).is_none());
    assert!(jsonl_line(&json!({"messages": []}), &json!({}), "m", Uuid::nil()).is_none());
}
