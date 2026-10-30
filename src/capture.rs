//! Capture the assistant output of a streamed response (ITEM-085).
//!
//! The stream tap already accumulates the upstream bytes to parse usage; this
//! module turns the same bytes into the completion the client actually
//! received, in the non-streaming response shape, so the dataset flywheel has
//! instruction pairs for streamed traffic (the dominant mode). Content and tool
//! calls are assembled from deltas; the capture is a bounded head, never
//! unbounded memory.

use serde_json::{json, Value};

use crate::sse::data_frames;

/// Most bytes of completion content one trace stores. A completion longer than
/// this is stored as a marked head (`capped: true`) rather than dropped.
pub const MAX_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    /// `{"choices": [{"message": {"role": "assistant", ...}}]}`.
    pub value: Value,
    /// The content hit [`MAX_BYTES`]; what is stored is a head.
    pub capped: bool,
}

/// Assemble the completion from an SSE body. `None` when nothing was served
/// (usage-only or empty frames).
pub fn from_sse_bytes(bytes: &[u8]) -> Option<Completion> {
    let mut content = String::new();
    let mut capped = false;
    let mut tools: Vec<Value> = Vec::new();
    for frame in data_frames(bytes) {
        let Some(choice) = frame["choices"].get(0) else {
            continue;
        };
        let delta = &choice["delta"];
        if let Some(text) = delta["content"].as_str() {
            let (head, truncated) = take_head(text, MAX_BYTES.saturating_sub(content.len()));
            content.push_str(head);
            capped |= truncated;
        }
        for call in delta["tool_calls"].as_array().into_iter().flatten() {
            let index = call["index"].as_u64().unwrap_or(0) as usize;
            while tools.len() <= index {
                tools.push(json!({"index": tools.len(), "type": "function",
                                  "function": {"name": "", "arguments": ""}}));
            }
            merge_tool(&mut tools[index], call);
        }
    }
    if content.is_empty() && tools.is_empty() {
        return None;
    }
    let mut message = json!({"role": "assistant", "content": content});
    if !tools.is_empty() {
        message["tool_calls"] = Value::Array(tools);
    }
    Some(Completion {
        value: json!({"choices": [{"message": message}]}),
        capped,
    })
}

/// The longest prefix of `text` that fits in `room` bytes without splitting a
/// character.
fn take_head(text: &str, room: usize) -> (&str, bool) {
    if text.len() <= room {
        return (text, false);
    }
    let mut end = room;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

/// Merge one streamed tool-call fragment into its accumulated slot: identity and
/// name are first-wins, arguments are appended.
fn merge_tool(slot: &mut Value, fragment: &Value) {
    if let Some(id) = fragment["id"].as_str() {
        if slot["id"].is_null() {
            slot["id"] = json!(id);
        }
    }
    if let Some(name) = fragment["function"]["name"].as_str() {
        if slot["function"]["name"].as_str().unwrap_or("").is_empty() {
            slot["function"]["name"] = json!(name);
        }
    }
    if let Some(arguments) = fragment["function"]["arguments"].as_str() {
        let current = slot["function"]["arguments"].as_str().unwrap_or("");
        let room = MAX_BYTES.saturating_sub(current.len());
        let (head, _) = take_head(arguments, room);
        slot["function"]["arguments"] = json!(format!("{current}{head}"));
    }
}

#[cfg(test)]
mod tests;
