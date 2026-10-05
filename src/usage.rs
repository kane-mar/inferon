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
