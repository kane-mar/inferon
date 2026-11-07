// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

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

use crate::sse::data_frames;

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
    for frame in data_frames(bytes) {
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
mod tests;
