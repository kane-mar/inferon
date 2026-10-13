//! Response quality classification (ITEM-055).
//!
//! The proxy already records what a request cost and how fast it was; this module
//! answers whether the response was actually **usable** — derived from the status
//! and body the proxy already holds, with no extra upstream call and no client
//! dependency. A client may attach its own thumbs up/down out of band (see
//! `POST /v1/quality`); this module only covers the derived signal.
//!
//! The classifier is deliberately shape-tolerant: it recognises both the
//! OpenAI (`choices`/`finish_reason`) and Anthropic (`content`/`stop_reason`)
//! envelopes, so one function serves both surfaces.

use serde_json::Value;

/// What happened to a request, from the caller's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// 2xx with usable content (text or a tool call).
    Ok,
    /// 2xx but the completion carried no content.
    Empty,
    /// 2xx but the model refused / the content filter stopped it.
    Refused,
    /// Non-2xx (upstream or proxy).
    Error,
    /// The response ended before the upstream finished (client abort or a
    /// mid-stream failure) — token counts are best-effort.
    Truncated,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Empty => "empty",
            Outcome::Refused => "refused",
            Outcome::Error => "error",
            Outcome::Truncated => "truncated",
        }
    }

    /// Parse a persisted outcome string; unknown or absent → `None`.
    pub fn from_db(value: Option<&str>) -> Option<Self> {
        match value? {
            "ok" => Some(Outcome::Ok),
            "empty" => Some(Outcome::Empty),
            "refused" => Some(Outcome::Refused),
            "error" => Some(Outcome::Error),
            "truncated" => Some(Outcome::Truncated),
            _ => None,
        }
    }
}

/// Classify a non-streaming response from its status and parsed body.
pub fn outcome_for_json(status: u16, body: &Option<Value>) -> Outcome {
    if status >= 400 {
        return Outcome::Error;
    }
    let Some(json) = body else {
        return Outcome::Empty;
    };
    if is_refusal(json) {
        return Outcome::Refused;
    }
    if has_content(json) {
        Outcome::Ok
    } else {
        Outcome::Empty
    }
}

/// Classify a streamed response from its status and accumulated SSE bytes.
pub fn outcome_for_sse(status: u16, bytes: &[u8]) -> Outcome {
    if status >= 400 {
        return Outcome::Error;
    }
    let mut saw_content = false;
    let mut refused = false;
    for line in bytes.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(data) = line.strip_prefix(b"data:") else { continue };
        let Ok(frame) = serde_json::from_slice::<Value>(trim_ascii_ws(data)) else {
            continue;
        };
        refused |= frame_is_refusal(&frame);
        saw_content |= frame_has_content(&frame);
    }
    if refused {
        Outcome::Refused
    } else if saw_content {
        Outcome::Ok
    } else {
        Outcome::Empty
    }
}

fn trim_ascii_ws(mut bytes: &[u8]) -> &[u8] {
    while let Some((&first, rest)) = bytes.split_first() {
        if first.is_ascii_whitespace() {
            bytes = rest;
        } else {
            break;
        }
    }
    while let Some((&last, rest)) = bytes.split_last() {
        if last.is_ascii_whitespace() {
            bytes = rest;
        } else {
            break;
        }
    }
    bytes
}

/// A model refusal / content-filter stop, in either envelope.
fn is_refusal(json: &Value) -> bool {
    choices(json).iter().any(|c| {
        c["finish_reason"].as_str() == Some("content_filter")
    }) || json["stop_reason"].as_str() == Some("refusal")
}

fn choices(json: &Value) -> &[Value] {
    json["choices"].as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// Usable content in a complete (non-streamed) envelope.
fn has_content(json: &Value) -> bool {
    for choice in choices(json) {
        if choice["message"]["content"].as_str().is_some_and(non_empty) {
            return true;
        }
        if choice["message"]["tool_calls"].as_array().is_some_and(|calls| !calls.is_empty()) {
            return true;
        }
    }
    for block in json["content"].as_array().into_iter().flatten() {
        match block["type"].as_str() {
            Some("text") if block["text"].as_str().is_some_and(non_empty) => return true,
            Some("tool_use") => return true,
            _ => {}
        }
    }
    false
}

/// Content in one streamed frame (delta), in either envelope.
fn frame_has_content(frame: &Value) -> bool {
    for choice in choices(frame) {
        if choice["delta"]["content"].as_str().is_some_and(non_empty) {
            return true;
        }
        if choice["delta"]["tool_calls"].as_array().is_some_and(|calls| !calls.is_empty()) {
            return true;
        }
    }
    match frame["type"].as_str() {
        Some("content_block_delta") => frame["delta"]["text"].as_str().is_some_and(non_empty),
        Some("content_block_start") => {
            frame["content_block"]["type"].as_str() == Some("tool_use")
        }
        _ => false,
    }
}

fn frame_is_refusal(frame: &Value) -> bool {
    choices(frame)
        .iter()
        .any(|c| c["finish_reason"].as_str() == Some("content_filter"))
        || frame["delta"]["stop_reason"].as_str() == Some("refusal")
}

fn non_empty(text: &str) -> bool {
    !text.trim().is_empty()
}

#[cfg(test)]
mod tests {
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
}
