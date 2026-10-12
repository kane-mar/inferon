//! Token-usage accounting parsed from upstream responses (streamed SSE and
//! non-streamed JSON). Pure parsing — no I/O, no state.

use serde_json::Value;

use crate::sha256_hex;

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

/// Token accounting parsed from an upstream response (usage object).
/// `Default` = no usage reached us (transport error / provider sent none).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub cached_tokens: Option<i64>,
}

/// Extract token counts from the last SSE data frame that carries a usage
/// object — e.g. DeepSeek's final chunk before [DONE]. This is run over the
/// already-accumulated stream bytes at stream end, so frames may be parsed
/// whole regardless of TCP fragmentation.
pub fn usage_from_sse_bytes(bytes: &[u8]) -> Option<Usage> {
    let mut last = None;
    for line in bytes.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(data) = line.strip_prefix(b"data:") else { continue };
        let data = trim_ascii_ws(data);
        if data == b"[DONE]" {
            continue;
        }
        let Ok(frame) = serde_json::from_slice::<Value>(data) else {
            continue;
        };
        if let (Some(p), Some(c)) = (
            frame["usage"]["prompt_tokens"].as_i64(),
            frame["usage"]["completion_tokens"].as_i64(),
        ) {
            last = Some(Usage {
                prompt_tokens: p as i32,
                completion_tokens: c as i32,
                cached_tokens: frame["usage"]["prompt_tokens_details"]["cached_tokens"]
                    .as_i64(),
            });
        }
    }
    last
}

/// SHA-256 hex of the first `role: system` message content (AC3).
pub(crate) fn system_prompt_hash(payload: &Value) -> Option<String> {
    payload["messages"]
        .as_array()?
        .iter()
        .find(|m| m["role"] == "system")
        .and_then(|m| m["content"].as_str())
        .map(sha256_hex)
}

/// Token counts from a parsed upstream JSON response (non-streaming path).
pub(crate) fn usage_from_json(response_json: &Option<Value>) -> Usage {
    match response_json {
        Some(json) => Usage {
            prompt_tokens: json["usage"]["prompt_tokens"].as_i64().unwrap_or(0) as i32,
            completion_tokens: json["usage"]["completion_tokens"].as_i64().unwrap_or(0) as i32,
            cached_tokens: json["usage"]["prompt_tokens_details"]["cached_tokens"].as_i64(),
        },
        None => Usage::default(),
    }
}

/// Token accounting from an Anthropic Messages response (`usage.input_tokens` /
/// `output_tokens`, cache reads in `cache_read_input_tokens`). ITEM-050.
pub fn usage_from_anthropic_json(response_json: &Option<Value>) -> Usage {
    match response_json {
        Some(json) => Usage {
            prompt_tokens: json["usage"]["input_tokens"].as_i64().unwrap_or(0) as i32,
            completion_tokens: json["usage"]["output_tokens"].as_i64().unwrap_or(0) as i32,
            cached_tokens: json["usage"]["cache_read_input_tokens"].as_i64(),
        },
        None => Usage::default(),
    }
}

/// Anthropic SSE carries usage across two frames: `message_start` has the input
/// usage, `message_delta` the final output usage. Run over the accumulated
/// stream bytes at stream end. ITEM-050.
pub fn usage_from_anthropic_sse_bytes(bytes: &[u8]) -> Option<Usage> {
    let mut prompt = None;
    let mut completion = None;
    let mut cached = None;
    for line in bytes.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(data) = line.strip_prefix(b"data:") else { continue };
        let data = trim_ascii_ws(data);
        let Ok(frame) = serde_json::from_slice::<Value>(data) else {
            continue;
        };
        match frame["type"].as_str() {
            Some("message_start") => {
                let usage = &frame["message"]["usage"];
                if let Some(v) = usage["input_tokens"].as_i64() {
                    prompt = Some(v as i32);
                }
                if let Some(v) = usage["output_tokens"].as_i64() {
                    completion = Some(v as i32);
                }
                if let Some(v) = usage["cache_read_input_tokens"].as_i64() {
                    cached = Some(v);
                }
            }
            Some("message_delta") => {
                let usage = &frame["usage"];
                if let Some(v) = usage["output_tokens"].as_i64() {
                    completion = Some(v as i32);
                }
                if let Some(v) = usage["cache_read_input_tokens"].as_i64() {
                    cached = Some(v);
                }
            }
            _ => {}
        }
    }
    prompt.map(|prompt_tokens| Usage {
        prompt_tokens,
        completion_tokens: completion.unwrap_or(0),
        cached_tokens: cached,
    })
}

/// SHA-256 hex of an Anthropic request's top-level `system` field — a string or
/// a list of text blocks. ITEM-050.
pub(crate) fn system_prompt_hash_anthropic(payload: &Value) -> Option<String> {
    let system = &payload["system"];
    if let Some(text) = system.as_str() {
        return (!text.is_empty()).then(|| sha256_hex(text));
    }
    let text: String = system
        .as_array()?
        .iter()
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then(|| sha256_hex(&text))
}

#[cfg(test)]
mod tests {
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
}
