    use super::*;
    use serde_json::json;

    fn openai(content: Value) -> Value {
        json!({"choices": [{"message": {"content": content}}]})
    }

    #[test]
    fn a_2xx_with_text_is_ok() {
        assert_eq!(outcome_for_json(200, &Some(openai(json!("hi")))), Outcome::Ok);
    }

    #[test]
    fn a_2xx_with_a_tool_call_is_ok_even_without_text() {
        let body = json!({"choices": [{"message": {"content": null, "tool_calls": [{"id": "call_1"}]}}]});
        assert_eq!(outcome_for_json(200, &Some(body)), Outcome::Ok);
    }

    #[test]
    fn a_2xx_with_blank_content_is_empty() {
        assert_eq!(outcome_for_json(200, &Some(openai(json!("   ")))), Outcome::Empty);
        assert_eq!(outcome_for_json(200, &Some(openai(json!(null)))), Outcome::Empty);
        assert_eq!(outcome_for_json(200, &None), Outcome::Empty);
    }

    #[test]
    fn a_content_filter_stop_is_refused() {
        let body = json!({"choices": [{"finish_reason": "content_filter", "message": {"content": ""}}]});
        assert_eq!(outcome_for_json(200, &Some(body)), Outcome::Refused);
    }

    #[test]
    fn anthropic_envelopes_are_understood() {
        let text = json!({"content": [{"type": "text", "text": "hi"}], "stop_reason": "end_turn"});
        assert_eq!(outcome_for_json(200, &Some(text)), Outcome::Ok);
        let tool = json!({"content": [{"type": "tool_use", "id": "tu_1"}], "stop_reason": "tool_use"});
        assert_eq!(outcome_for_json(200, &Some(tool)), Outcome::Ok);
        let refused = json!({"content": [], "stop_reason": "refusal"});
        assert_eq!(outcome_for_json(200, &Some(refused)), Outcome::Refused);
        let empty = json!({"content": [], "stop_reason": "end_turn"});
        assert_eq!(outcome_for_json(200, &Some(empty)), Outcome::Empty);
    }

    #[test]
    fn a_non_2xx_is_an_error_regardless_of_body() {
        assert_eq!(outcome_for_json(500, &Some(openai(json!("boom")))), Outcome::Error);
        assert_eq!(outcome_for_json(429, &None), Outcome::Error);
    }

    #[test]
    fn openai_sse_content_delta_is_ok() {
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: [DONE]\n\n";
        assert_eq!(outcome_for_sse(200, sse.as_bytes()), Outcome::Ok);
    }

    #[test]
    fn an_sse_stream_with_only_usage_is_empty() {
        let sse = "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":0}}\n\ndata: [DONE]\n\n";
        assert_eq!(outcome_for_sse(200, sse.as_bytes()), Outcome::Empty);
    }

    #[test]
    fn a_streamed_content_filter_is_refused() {
        let sse = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"content_filter\"}]}\n\ndata: [DONE]\n\n";
        assert_eq!(outcome_for_sse(200, sse.as_bytes()), Outcome::Refused);
    }

    #[test]
    fn anthropic_sse_content_and_refusal_are_understood() {
        let content = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n";
        assert_eq!(outcome_for_sse(200, content.as_bytes()), Outcome::Ok);
        let refusal = "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"refusal\"}}\n\n";
        assert_eq!(outcome_for_sse(200, refusal.as_bytes()), Outcome::Refused);
    }

    #[test]
    fn a_non_2xx_stream_is_an_error() {
        assert_eq!(outcome_for_sse(502, b"data: {}\n\n"), Outcome::Error);
    }

    #[test]
    fn outcome_strings_round_trip() {
        for outcome in [
            Outcome::Ok,
            Outcome::Empty,
            Outcome::Refused,
            Outcome::Error,
            Outcome::Truncated,
        ] {
            assert_eq!(Outcome::from_db(Some(outcome.as_str())), Some(outcome));
        }
        assert_eq!(Outcome::from_db(None), None);
        assert_eq!(Outcome::from_db(Some("garbage")), None);
    }
