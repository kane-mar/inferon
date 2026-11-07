// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

    use super::*;
    use serde_json::json;

    #[test]
    fn anthropic_json_usage_reads_input_output_and_cache() {
        let usage = usage_from_anthropic_json(&Some(json!({
            "usage": {"input_tokens": 120, "output_tokens": 40, "cache_read_input_tokens": 100}
        })));
        assert_eq!(usage.prompt_tokens, 120);
        assert_eq!(usage.completion_tokens, 40);
        assert_eq!(usage.cached_tokens, Some(100));
        assert_eq!(usage_from_anthropic_json(&None), Usage::default());
    }

    #[test]
    fn anthropic_sse_usage_spans_message_start_and_delta() {
        let body = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":50,\"output_tokens\":1,\"cache_read_input_tokens\":30}}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"hi\"}}\n\n\
event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":9}}\n\n";
        let usage = usage_from_anthropic_sse_bytes(body.as_bytes()).expect("usage parsed");
        assert_eq!(usage.prompt_tokens, 50);
        assert_eq!(usage.completion_tokens, 9, "the final output total wins");
        assert_eq!(usage.cached_tokens, Some(30));
        // No message_start -> no usage (never a false zero).
        assert_eq!(usage_from_anthropic_sse_bytes(b"data: [DONE]\n\n"), None);
    }

    #[test]
    fn anthropic_system_hash_handles_string_and_blocks() {
        let s = json!({"system": "You are terse."});
        assert_eq!(system_prompt_hash_anthropic(&s), Some(sha256_hex("You are terse.")));
        let blocks = json!({"system": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]});
        assert_eq!(system_prompt_hash_anthropic(&blocks), Some(sha256_hex("a\nb")));
        assert_eq!(system_prompt_hash_anthropic(&json!({})), None);
        assert_eq!(system_prompt_hash_anthropic(&json!({"system": ""})), None);
    }
