// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

#[test]
fn openai_text_deltas_become_anthropic_message_events() {
    let mut stream = OpenAiToAnthropic::new("deepseek-flash", "msg_1");
    let mut out = String::new();
    out.push_str(&text(&stream.push(
        b"data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"role\":\"assistant\"}}],\"usage\":{\"prompt_tokens\":9}}\n\n",
    )));
    out.push_str(&text(&stream.push(
        b"data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
    )));
    out.push_str(&text(&stream.push(
        b"data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
    )));
    out.push_str(&text(&stream.push(
        b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"completion_tokens\":2}}\n\n",
    )));
    out.push_str(&text(&stream.push(b"data: [DONE]\n\n")));

    assert!(out.contains("event: message_start"), "{out}");
    assert!(out.contains("\"input_tokens\":9"), "{out}");
    assert_eq!(out.matches("event: content_block_delta").count(), 2, "one per text delta");
    assert!(out.contains("\"text\":\"Hel\""), "{out}");
    assert!(out.contains("\"text\":\"lo\""), "{out}");
    assert!(out.contains("\"stop_reason\":\"end_turn\""), "{out}");
    assert!(out.contains("\"output_tokens\":2"), "{out}");
    assert!(out.contains("event: message_stop"), "{out}");
}

#[test]
fn an_openai_tool_call_becomes_a_tool_use_block_with_partial_json() {
    let mut stream = OpenAiToAnthropic::new("deepseek-flash", "msg_2");
    let mut out = String::new();
    out.push_str(&text(&stream.push(
        b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"calc\",\"arguments\":\"\"}}]}}]}\n\n",
    )));
    out.push_str(&text(&stream.push(
        b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"x\\\":1}\"}}]}}]}\n\n",
    )));
    out.push_str(&text(&stream.push(
        b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n",
    )));

    assert!(out.contains("\"type\":\"tool_use\""), "{out}");
    assert!(out.contains("\"name\":\"calc\""), "{out}");
    assert!(out.contains("input_json_delta"), "{out}");
    assert!(out.contains("\"stop_reason\":\"tool_use\""), "{out}");
}

#[test]
fn frames_split_across_chunks_are_reassembled() {
    let mut stream = OpenAiToAnthropic::new("m", "i");
    let first = text(&stream.push(b"data: {\"choices\":[{\"delta\":{\"cont"));
    assert!(first.is_empty(), "no complete line yet: {first}");
    let second = text(&stream.push(b"ent\":\"hi\"}}]}\n\ndata: [DONE]\n\n"));
    assert!(second.contains("\"text\":\"hi\""), "{second}");
    assert!(second.contains("message_stop"), "{second}");
}

#[test]
fn finish_is_idempotent_and_closes_the_message() {
    let mut stream = OpenAiToAnthropic::new("m", "i");
    let first = stream.finish();
    assert!(first.contains("message_delta") && first.contains("message_stop"));
    assert!(stream.finish().is_empty(), "a second [DONE] adds nothing");
}

#[test]
fn a_mid_stream_upstream_error_maps_to_the_target_shape() {
    let mut stream = OpenAiToAnthropic::new("m", "i");
    let out = text(&stream.push(b"data: {\"error\":{\"message\":\"overloaded\"}}\n\n"));
    assert!(out.contains("event: error"), "{out}");
    assert!(out.contains("overloaded"), "{out}");
    assert!(out.contains("message_stop"), "the stream is still closed");
}

#[test]
fn anthropic_text_events_become_openai_deltas() {
    let mut stream = AnthropicToOpenAi::new("claude", "chatcmpl-1");
    let mut out = String::new();
    out.push_str(&text(&stream.push(
        b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":7}}}\n\n",
    )));
    out.push_str(&text(&stream.push(
        b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n",
    )));
    out.push_str(&text(&stream.push(
        b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
    )));
    out.push_str(&text(&stream.push(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n")));

    assert!(out.contains("\"role\":\"assistant\""), "{out}");
    assert!(out.contains("\"content\":\"Hi\""), "{out}");
    assert!(out.contains("\"finish_reason\":\"stop\""), "{out}");
    assert!(out.contains("\"prompt_tokens\":7"), "{out}");
    assert!(out.contains("\"completion_tokens\":1"), "{out}");
    assert!(out.trim_end().ends_with("[DONE]"), "{out}");
}

#[test]
fn an_anthropic_tool_block_becomes_openai_tool_call_deltas() {
    let mut stream = AnthropicToOpenAi::new("claude", "chatcmpl-2");
    let mut out = String::new();
    out.push_str(&text(&stream.push(
        b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"calc\",\"input\":{}}}\n\n",
    )));
    out.push_str(&text(&stream.push(
        b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"x\\\":\"}}\n\n",
    )));
    out.push_str(&text(&stream.push(
        b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"1}\"}}\n\n",
    )));
    out.push_str(&text(&stream.push(
        b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n",
    )));

    assert!(out.contains("\"id\":\"t1\""), "{out}");
    assert!(out.contains("\"name\":\"calc\""), "{out}");
    assert!(out.contains("{\\\"x\\\":"), "{out}");
    assert!(out.contains("\"finish_reason\":\"tool_calls\""), "{out}");
}
