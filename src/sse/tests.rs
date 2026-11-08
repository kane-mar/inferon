// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

    use super::*;

    #[test]
    fn data_frames_skips_done_and_non_json() {
        let sse = "event: ping\ndata: {\"a\":1}\n\ndata: not json\n\ndata: [DONE]\n\n";
        let frames: Vec<Value> = data_frames(sse.as_bytes()).collect();
        assert_eq!(frames, vec![serde_json::json!({"a": 1})]);
    }

    #[test]
    fn data_frames_tolerates_crlf_and_padding() {
        let sse = "data:   {\"a\": 2}  \r\n\r\n";
        let frames: Vec<Value> = data_frames(sse.as_bytes()).collect();
        assert_eq!(frames, vec![serde_json::json!({"a": 2})]);
    }

    // ITEM-130: how a stream is allowed to end. A client accepts an answer only
    // when it can tell a finished answer from a severed one, so the tail is
    // decided from the bytes the client is about to have read.

    fn text(tail: &StreamTail) -> String {
        String::from_utf8_lossy(&tail.bytes).into_owned()
    }

    #[test]
    fn a_stream_that_says_why_it_stopped_needs_no_tail() {
        let sse = "data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        assert!(tail(sse.as_bytes(), false, Surface::OpenAi, "trace-1", "m").is_none());
    }

    #[test]
    fn a_stream_that_stops_mid_answer_is_named_and_closed() {
        let sse = "data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n";
        let tail = tail(sse.as_bytes(), false, Surface::OpenAi, "trace-1", "m").expect("a tail");
        let body = text(&tail);
        assert!(body.contains("\"code\":\"stream_ended_early\""), "{body}");
        assert!(body.contains("trace-1"), "the failure names its trace: {body}");
        assert!(body.ends_with("data: [DONE]\n\n"), "the body still ends: {body}");
        assert!(tail.truncated, "a cut stream is a truncated trace");
    }

    #[test]
    fn an_empty_stream_is_a_failure_with_a_name() {
        let tail = tail(b"data: [DONE]\n\n", false, Surface::OpenAi, "trace-1", "m").expect("a tail");
        let body = text(&tail);
        assert!(body.contains("\"code\":\"empty_answer\""), "{body}");
        assert!(!tail.truncated, "the provider did finish; it just said nothing");
    }

    #[test]
    fn a_transport_failure_reaches_the_client_as_an_error() {
        let sse = "data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n";
        let tail = tail(sse.as_bytes(), true, Surface::OpenAi, "trace-1", "m").expect("a tail");
        assert!(text(&tail).contains("\"code\":\"upstream_stream_error\""));
        assert!(tail.truncated);
    }

    #[test]
    fn a_complete_answer_without_its_terminal_frame_gets_one() {
        // The provider closed a finished answer without saying so. The answer
        // earned a `stop`, so say it, and reuse the provider's own id.
        let sse = "data: {\"id\":\"chatcmpl-7\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: [DONE]\n\n";
        let tail = tail(sse.as_bytes(), false, Surface::OpenAi, "trace-1", "deepseek-flash")
            .expect("a tail");
        let body = text(&tail);
        assert!(body.contains("\"finish_reason\":\"stop\""), "{body}");
        assert!(body.contains("\"id\":\"chatcmpl-7\""), "keeps the stream's id: {body}");
        assert!(body.contains("\"model\":\"deepseek-flash\""), "{body}");
        assert!(!tail.truncated, "the answer arrived in full");
    }

    #[test]
    fn an_anthropic_client_is_told_in_its_own_protocol() {
        let sse = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n";
        let tail = tail(sse.as_bytes(), false, Surface::Anthropic, "trace-1", "m").expect("a tail");
        let body = text(&tail);
        assert!(body.starts_with("event: error\n"), "{body}");
        assert!(body.contains("\"type\":\"error\""), "{body}");
        assert!(body.contains("trace-1"), "{body}");
    }

    #[test]
    fn an_anthropic_stream_that_ended_cleanly_needs_no_tail() {
        let sse = "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
        assert!(tail(sse.as_bytes(), false, Surface::Anthropic, "trace-1", "m").is_none());
    }

    // ITEM-131: "has this stream started saying anything" decides whether an
    // attempt may still be abandoned. Reasoning and tool calls count — buffering
    // them would trade a silent failure for a latency regression.

    #[test]
    fn a_reasoning_delta_counts_as_speaking() {
        let sse = "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"think\"}}]}\n\n";
        assert!(saying_something(sse.as_bytes()));
    }

    #[test]
    fn a_tool_call_counts_as_speaking() {
        let sse = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\"}]}}]}\n\n";
        assert!(saying_something(sse.as_bytes()));
        let anthropic = "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"a\\\"\"}}\n\n";
        assert!(saying_something(anthropic.as_bytes()));
    }

    #[test]
    fn silence_is_silence() {
        // A role frame, a usage frame and the sentinel say nothing a client reads.
        let role = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":null}}]}\n\n";
        assert!(!saying_something(role.as_bytes()));
        let usage = "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5}}\n\ndata: [DONE]\n\n";
        assert!(!saying_something(usage.as_bytes()));
        assert!(!saying_something(b""));
    }
