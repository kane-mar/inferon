//! Token-usage accounting parsed from upstream responses (streamed SSE and
//! non-streamed JSON). Pure parsing — no I/O, no state.

use serde_json::Value;

use crate::sha256_hex;
use crate::sse::data_frames;

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
    for frame in data_frames(bytes) {
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
    for frame in data_frames(bytes) {
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
mod tests;
