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
