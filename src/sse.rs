// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! SSE frame parsing shared by usage and quality extraction.
//!
//! Both `usage` and `quality` walk an accumulated SSE buffer looking only at
//! `data:` payloads; this keeps that walk in one place.
//! It also decides how a stream may end (ITEM-130): a client accepts an answer
//! only when it can tell a finished answer from a cut one.

use crate::proxy::route::Surface;
use serde_json::Value;

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

/// The JSON payloads of every `data:` line in an accumulated SSE buffer,
/// skipping the `[DONE]` sentinel and any non-JSON frame.
pub fn data_frames(bytes: &[u8]) -> impl Iterator<Item = Value> + '_ {
    bytes.split(|&b| b == b'\n').filter_map(|line| {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let data = trim_ascii_ws(line.strip_prefix(b"data:")?);
        if data == b"[DONE]" {
            return None;
        }
        serde_json::from_slice::<Value>(data).ok()
    })
}

/// Whether these bytes already carry something a client will use (ITEM-131).
///
/// The test is deliberately *not* `quality`'s "is this answer usable": the
/// question here is "has this stream started saying anything", so a reasoning
/// delta or a tool call counts. A provider that streams 10 s of
/// `reasoning_content` before its first `content` chunk must not be buffered for
/// those 10 s — that would trade one defect for a latency regression.
pub(crate) fn saying_something(raw: &[u8]) -> bool {
    data_frames(raw).any(|frame| {
        let choice = frame["choices"].as_array().and_then(|c| c.first());
        if let Some(delta) = choice.map(|c| &c["delta"]) {
            const FIELDS: [&str; 3] = ["content", "reasoning_content", "function_call"];
            if FIELDS
                .iter()
                .any(|f| delta[f].as_str().is_some_and(|text| !text.is_empty()))
            {
                return true;
            }
            if delta["tool_calls"].as_array().is_some_and(|c| !c.is_empty()) {
                return true;
            }
        }
        // The Anthropic envelope, translated after this tap.
        match frame["type"].as_str() {
            Some("content_block_delta") => ["text", "partial_json", "thinking"]
                .iter()
                .any(|f| frame["delta"][f].as_str().is_some_and(|text| !text.is_empty())),
            Some("content_block_start") => {
                matches!(
                    frame["content_block"]["type"].as_str(),
                    Some("tool_use" | "text" | "thinking")
                )
            }
            _ => false,
        }
    })
}

/// The bytes to append to a stream that did not end the way a client needs, and
/// whether the trace must be marked truncated.
pub(crate) struct StreamTail {
    pub(crate) bytes: Vec<u8>,
    pub(crate) truncated: bool,
}

/// How a stream ended, from the bytes the client is about to have read.
///
/// ITEM-130: a body that simply stops is unusable. An SDK sees no
/// `finish_reason`, cannot tell a finished answer from a severed one, and raises
/// a message that names nothing you can look up ("stream ended without
/// finish_reason"). So the tail is decided here, once, from what actually
/// arrived, and the client is told in its own protocol.
#[derive(Debug, PartialEq, Eq)]
enum Ending {
    /// A terminal frame arrived; the stream speaks for itself.
    Finished,
    /// The answer arrived but no terminal frame did.
    Answered,
    /// The stream ended cleanly and carries no answer.
    Empty,
    /// The body ended before the provider said it was done.
    Cut,
}

/// Append exactly what this stream needs to be trustworthy, or nothing.
///
/// `errored` marks a body whose transport failed mid-flight. `trace_id` goes
/// into the error the client sees, so a report of the failure can be looked up.
pub(crate) fn tail(
    raw: &[u8],
    errored: bool,
    surface: Surface,
    trace_id: &str,
    model: &str,
) -> Option<StreamTail> {
    if errored {
        return Some(StreamTail {
            truncated: true,
            ..error_tail(
                surface,
                trace_id,
                "upstream_stream_error",
                "the provider's connection dropped before the answer was complete",
            )
        });
    }
    let ending = ending(raw, surface);
    match ending {
        Ending::Finished => None,
        // The provider closed a complete answer without its terminal frame. The
        // client asked for a finish_reason, so give it the one the stream earned
        // rather than let the answer look severed.
        Ending::Answered => Some(finish_tail(raw, surface, model)),
        Ending::Empty => Some(error_tail(
            surface,
            trace_id,
            "empty_answer",
            "the provider finished the stream without an answer",
        )),
        Ending::Cut => Some(StreamTail {
            truncated: true,
            ..error_tail(
                surface,
                trace_id,
                "stream_ended_early",
                "the provider ended the stream before the answer was complete",
            )
        }),
    }
}

/// Classify the accumulated bytes for the surface the client speaks.
fn ending(raw: &[u8], surface: Surface) -> Ending {
    match surface {
        Surface::OpenAi => {
            let mut answered = false;
            let mut saw_finish = false;
            for frame in data_frames(raw) {
                saw_finish |= frame["choices"]
                    .as_array()
                    .is_some_and(|choices| choices.iter().any(|c| c["finish_reason"].is_string()));
                answered |= !frame_content(&frame).is_empty();
            }
            let saw_done = raw
                .split(|&b| b == b'\n')
                .any(|line| trim_ascii_ws(line.strip_prefix(b"data:").unwrap_or(b"")) == b"[DONE]");
            match (saw_finish, saw_done, answered) {
                (true, _, _) => Ending::Finished,
                (false, false, _) => Ending::Cut,
                (false, true, true) => Ending::Answered,
                (false, true, false) => Ending::Empty,
            }
        }
        Surface::Anthropic => {
            let mut stopped = false;
            let mut answered = false;
            for line in raw.split(|&b| b == b'\n') {
                let line = line.strip_suffix(b"\r").unwrap_or(line);
                let Some(data) = line.strip_prefix(b"data:") else {
                    continue;
                };
                let Ok(frame) = serde_json::from_slice::<Value>(trim_ascii_ws(data)) else {
                    continue;
                };
                match frame["type"].as_str() {
                    // The end of the message, or the reason it stopped.
                    Some("message_stop") => stopped = true,
                    Some("message_delta") => {
                        stopped |= frame["delta"]["stop_reason"].is_string();
                    }
                    Some("content_block_delta") => {
                        let delta = &frame["delta"];
                        answered |= delta["text"].as_str().is_some_and(|t| !t.is_empty())
                            || delta["partial_json"].is_string();
                    }
                    _ => {}
                }
            }
            match (stopped, answered) {
                (true, _) => Ending::Finished,
                (false, true) => Ending::Cut,
                (false, false) => Ending::Empty,
            }
        }
    }
}
/// Assistant text in an OpenAI frame, from either answer shape.
fn frame_content(frame: &Value) -> &str {
    let choice = frame["choices"].as_array().and_then(|c| c.first());
    choice
        .and_then(|c| c["delta"]["content"].as_str())
        .or_else(|| choice.and_then(|c| c["message"]["content"].as_str()))
        .unwrap_or("")
}

/// The terminal chunk a provider owed us: no content, just the reason.
fn finish_tail(raw: &[u8], surface: Surface, model: &str) -> StreamTail {
    let mut id = String::new();
    for frame in data_frames(raw) {
        if let Some(existing) = frame["id"].as_str() {
            id = existing.to_string();
            break;
        }
    }
    let bytes = match surface {
        Surface::OpenAi => {
            let mut frame = serde_json::json!({
                "object": "chat.completion.chunk",
                "model": model,
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            });
            if !id.is_empty() {
                frame["id"] = Value::String(id);
            }
            format!("data: {frame}\n\ndata: [DONE]\n\n").into_bytes()
        }
        // A translated Anthropic stream ends with `message_stop`, which the
        // translator already emits; an OpenAI provider owes us nothing extra.
        Surface::Anthropic => format!(
            "event: message_stop\ndata: {}\n\n",
            serde_json::json!({"type": "message_stop"})
        )
        .into_bytes(),
    };
    StreamTail {
        bytes,
        truncated: false,
    }
}

/// A typed failure the client's own SDK will surface, naming the trace.
fn error_tail(surface: Surface, trace_id: &str, code: &str, message: &str) -> StreamTail {
    let message = format!("{message} (request id {trace_id})");
    let bytes = match surface {
        Surface::OpenAi => {
            let frame = serde_json::json!({
                "error": {
                    "message": message,
                    "type": "upstream_error",
                    "param": null,
                    "code": code,
                }
            });
            format!("data: {frame}\n\ndata: [DONE]\n\n").into_bytes()
        }
        Surface::Anthropic => {
            let frame = serde_json::json!({
                "type": "error",
                "error": {"type": "api_error", "message": message},
            });
            format!("event: error\ndata: {frame}\n\n").into_bytes()
        }
    };
    StreamTail {
        bytes,
        truncated: false,
    }
}

#[cfg(test)]
mod tests;
