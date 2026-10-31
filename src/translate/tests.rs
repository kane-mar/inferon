use super::*;

#[test]
fn an_anthropic_request_becomes_chat_completions() {
    let request = json!({
        "model": "claude-sonnet-4",
        "max_tokens": 512,
        "temperature": 0.2,
        "stop_sequences": ["\n\nHuman:"],
        "system": "be terse",
        "messages": [
            {"role": "user", "content": "hello"},
            {"role": "assistant", "content": "hi"},
            {"role": "user", "content": [{"type": "text", "text": "extract the total"}]}
        ]
    });
    let translated = request_to_openai(&request).expect("translated");
    assert_eq!(translated["model"], "claude-sonnet-4");
    assert_eq!(translated["messages"][0], json!({"role": "system", "content": "be terse"}));
    assert_eq!(translated["messages"][1], json!({"role": "user", "content": "hello"}));
    assert_eq!(translated["messages"][3], json!({"role": "user", "content": "extract the total"}));
    assert_eq!(translated["max_tokens"], 512);
    assert_eq!(translated["stop"], json!(["\n\nHuman:"]));
    assert_eq!(translated["temperature"], 0.2);
}

#[test]
fn anthropic_tools_become_function_definitions_and_choices() {
    let request = json!({
        "model": "claude",
        "system": [{"type": "text", "text": "one"}, {"type": "text", "text": "two"}],
        "messages": [{"role": "user", "content": "q"}],
        "tools": [{"name": "lookup", "description": "find", "input_schema": {"type": "object"}}],
        "tool_choice": {"type": "tool", "name": "lookup"}
    });
    let translated = request_to_openai(&request).expect("translated");
    assert_eq!(translated["messages"][0]["content"], "one\ntwo", "system blocks join");
    assert_eq!(translated["tools"][0]["type"], "function");
    assert_eq!(translated["tools"][0]["function"]["name"], "lookup");
    assert_eq!(translated["tools"][0]["function"]["parameters"]["type"], "object");
    assert_eq!(translated["tool_choice"]["function"]["name"], "lookup");
}

#[test]
fn anthropic_tool_use_and_results_map_to_their_own_messages() {
    let request = json!({
        "model": "claude",
        "messages": [
            {"role": "user", "content": "what is 2+2?"},
            {"role": "assistant", "content": [
                {"type": "text", "text": "let me look"},
                {"type": "tool_use", "id": "t1", "name": "calc", "input": {"expr": "2+2"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "4"}]}
            ]},
            {"role": "user", "content": "thanks"}
        ]
    });
    let translated = request_to_openai(&request).expect("translated");
    let messages = translated["messages"].as_array().unwrap();
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["tool_calls"][0]["id"], "t1");
    assert_eq!(messages[1]["tool_calls"][0]["function"]["arguments"], "{\"expr\":\"2+2\"}");
    assert_eq!(messages[2], json!({"role": "tool", "tool_call_id": "t1", "content": "4"}));
    assert_eq!(messages[3], json!({"role": "user", "content": "thanks"}));
}

#[test]
fn an_image_block_is_refused_by_field_not_dropped() {
    let request = json!({
        "model": "claude",
        "messages": [{"role": "user", "content": [
            {"type": "text", "text": "what is this"},
            {"type": "image", "source": {"type": "base64", "data": "..."}}
        ]}]
    });
    let error = request_to_openai(&request).expect_err("images have no text equivalent");
    assert_eq!(error.field, "messages[0].content[1].image");
    assert!(error.detail.contains("text only"));
}

#[test]
fn an_openai_completion_becomes_an_anthropic_message() {
    let response = json!({
        "id": "chatcmpl-1",
        "model": "deepseek-flash",
        "choices": [{"index": 0, "finish_reason": "length",
                     "message": {"role": "assistant", "content": "ok"}}],
        "usage": {"prompt_tokens": 12, "completion_tokens": 3}
    });
    let translated = response_to_anthropic(&response).expect("translated");
    assert_eq!(translated["type"], "message");
    assert_eq!(translated["role"], "assistant");
    assert_eq!(translated["content"][0], json!({"type": "text", "text": "ok"}));
    assert_eq!(translated["stop_reason"], "max_tokens");
    assert_eq!(translated["usage"]["input_tokens"], 12);
    assert_eq!(translated["usage"]["output_tokens"], 3);
}

#[test]
fn openai_tool_calls_become_anthropic_tool_use_blocks() {
    let response = json!({
        "id": "chatcmpl-2",
        "model": "deepseek-flash",
        "choices": [{"index": 0, "finish_reason": "tool_calls",
                     "message": {"role": "assistant", "content": null, "tool_calls": [{
                         "id": "call_1", "type": "function",
                         "function": {"name": "calc", "arguments": "{\"expr\":\"2+2\"}"}
                     }]}}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 2}
    });
    let translated = response_to_anthropic(&response).expect("translated");
    assert_eq!(translated["stop_reason"], "tool_use");
    assert_eq!(translated["content"][0]["type"], "tool_use");
    assert_eq!(translated["content"][0]["id"], "call_1");
    assert_eq!(translated["content"][0]["input"]["expr"], "2+2");
}

#[test]
fn an_openai_request_becomes_an_anthropic_messages_request() {
    let request = json!({
        "model": "gpt-4o",
        "max_tokens": 256,
        "stop": ["END"],
        "messages": [
            {"role": "system", "content": "be terse"},
            {"role": "user", "content": [{"type": "text", "text": "hello"}]},
            {"role": "assistant", "content": null, "tool_calls": [
                {"id": "c1", "type": "function", "function": {"name": "calc", "arguments": "{\"x\":1}"}}
            ]},
            {"role": "tool", "tool_call_id": "c1", "content": "2"},
            {"role": "tool", "tool_call_id": "c1", "content": "3"}
        ],
        "tools": [{"type": "function", "function": {"name": "calc", "description": "add"}}],
        "tool_choice": "required"
    });
    let translated = request_to_anthropic(&request).expect("translated");
    assert_eq!(translated["system"], "be terse");
    let messages = translated["messages"].as_array().unwrap();
    assert_eq!(messages[0]["content"][0], json!({"type": "text", "text": "hello"}));
    assert_eq!(messages[1]["content"][0]["type"], "tool_use");
    assert_eq!(messages[1]["content"][0]["input"]["x"], 1);
    // Consecutive tool results share one user turn, as Anthropic expects.
    assert_eq!(messages[2]["content"].as_array().unwrap().len(), 2);
    assert_eq!(messages[2]["content"][1]["content"], "3");
    assert_eq!(translated["max_tokens"], 256);
    assert_eq!(translated["stop_sequences"], json!(["END"]));
    assert_eq!(translated["tools"][0]["input_schema"]["type"], "object");
    assert_eq!(translated["tool_choice"], json!({"type": "any"}));
}

#[test]
fn a_missing_max_tokens_gets_the_documented_default() {
    let request = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let translated = request_to_anthropic(&request).expect("translated");
    assert_eq!(translated["max_tokens"], DEFAULT_MAX_TOKENS);
}

#[test]
fn an_anthropic_message_becomes_a_chat_completion() {
    let response = json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "claude",
        "content": [
            {"type": "text", "text": "hello "},
            {"type": "text", "text": "world"},
            {"type": "tool_use", "id": "t1", "name": "calc", "input": {"x": 2}}
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 7, "output_tokens": 4}
    });
    let translated = response_to_openai(&response).expect("translated");
    let choice = &translated["choices"][0];
    assert_eq!(choice["message"]["content"], "hello world");
    assert_eq!(choice["finish_reason"], "tool_calls");
    assert_eq!(choice["message"]["tool_calls"][0]["function"]["arguments"], "{\"x\":2}");
    assert_eq!(translated["usage"]["prompt_tokens"], 7);
    assert_eq!(translated["usage"]["total_tokens"], 11);
}

#[test]
fn stop_reasons_map_both_ways() {
    assert_eq!(stop_reason_to_anthropic(Some("stop")), "end_turn");
    assert_eq!(stop_reason_to_anthropic(Some("length")), "max_tokens");
    assert_eq!(stop_reason_to_anthropic(Some("tool_calls")), "tool_use");
    assert_eq!(stop_reason_to_anthropic(None), "end_turn");
    assert_eq!(finish_reason_to_openai(Some("end_turn")), "stop");
    assert_eq!(finish_reason_to_openai(Some("stop_sequence")), "stop");
    assert_eq!(finish_reason_to_openai(Some("max_tokens")), "length");
    assert_eq!(finish_reason_to_openai(Some("tool_use")), "tool_calls");
}

#[test]
fn an_openai_image_part_is_refused_by_field() {
    let request = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": [
            {"type": "image_url", "image_url": {"url": "https://example.test/x.png"}}
        ]}]
    });
    let error = request_to_anthropic(&request).expect_err("images have no text equivalent");
    assert_eq!(error.field, "messages[0].content[0].image_url");
}

#[test]
fn a_response_without_choices_is_a_typed_error() {
    let error = response_to_anthropic(&json!({"id": "x", "choices": []})).expect_err("no choices");
    assert_eq!(error.field, "choices");
}
