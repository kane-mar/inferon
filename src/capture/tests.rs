// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;
use serde_json::json;

fn sse(frames: &[Value]) -> Vec<u8> {
    let mut body = String::new();
    for frame in frames {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    body.into_bytes()
}

#[test]
fn content_deltas_are_concatenated() {
    let bytes = sse(&[
        json!({"choices": [{"delta": {"content": "Hello"}}]}),
        json!({"choices": [{"delta": {"content": ", world"}}]}),
        json!({"choices": [{"delta": {}, "finish_reason": "stop"}]}),
    ]);
    let captured = from_sse_bytes(&bytes).expect("a completion");
    assert!(!captured.capped);
    let message = &captured.value["choices"][0]["message"];
    assert_eq!(message["role"], "assistant");
    assert_eq!(message["content"], "Hello, world");
}

#[test]
fn tool_call_fragments_merge_by_index() {
    let bytes = sse(&[
        json!({"choices": [{"delta": {"tool_calls": [
            {"index": 0, "id": "call_1", "type": "function",
             "function": {"name": "lookup", "arguments": "{\"q\":"}}
        ]}}]}),
        json!({"choices": [{"delta": {"tool_calls": [
            {"index": 0, "function": {"arguments": "\"x\"}"}}
        ]}}]}),
    ]);
    let captured = from_sse_bytes(&bytes).expect("a completion");
    let call = &captured.value["choices"][0]["message"]["tool_calls"][0];
    assert_eq!(call["id"], "call_1");
    assert_eq!(call["function"]["name"], "lookup");
    assert_eq!(call["function"]["arguments"], "{\"q\":\"x\"}");
}

#[test]
fn a_huge_completion_is_capped_not_dropped() {
    let chunk = "x".repeat(MAX_BYTES / 2);
    let bytes = sse(&[
        json!({"choices": [{"delta": {"content": chunk}}]}),
        json!({"choices": [{"delta": {"content": chunk}}]}),
        json!({"choices": [{"delta": {"content": chunk}}]}),
    ]);
    let captured = from_sse_bytes(&bytes).expect("a completion");
    assert!(captured.capped);
    let content = captured.value["choices"][0]["message"]["content"]
        .as_str()
        .unwrap();
    assert_eq!(content.len(), MAX_BYTES);
}

#[test]
fn a_cap_never_splits_a_utf8_character() {
    let content = "é".repeat(MAX_BYTES); // two bytes each
    let bytes = sse(&[json!({"choices": [{"delta": {"content": content}}]})]);
    let captured = from_sse_bytes(&bytes).expect("a completion");
    assert!(captured.capped);
    let stored = captured.value["choices"][0]["message"]["content"]
        .as_str()
        .unwrap();
    assert!(stored.len() <= MAX_BYTES);
    assert!(stored.chars().all(|c| c == 'é'), "no replacement characters");
}

#[test]
fn usage_only_and_empty_frames_capture_nothing() {
    assert!(from_sse_bytes(&sse(&[json!({"choices": [{"delta": {}}]})])).is_none());
    assert!(from_sse_bytes(&sse(&[json!({"usage": {"prompt_tokens": 3}})])).is_none());
}
