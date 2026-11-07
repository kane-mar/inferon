// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! Streaming translators (ITEM-094 AC4).
//!
//! Chunks arrive at arbitrary byte boundaries, so both directions buffer until a
//! whole SSE line is available. Each push returns the bytes to send to the
//! client; `finish` closes the stream (usage and stop reason included) so a
//! client never sees a truncated message.

use serde_json::{json, Value};

use super::{finish_reason_to_openai, stop_reason_to_anthropic};

/// `text/event-stream` frame.
fn frame(event: Option<&str>, data: &Value) -> String {
    match event {
        Some(event) => format!("event: {event}\ndata: {data}\n\n"),
        None => format!("data: {data}\n\n"),
    }
}

/// Read complete `data:` lines out of the buffer, leaving any partial line.
fn drain(buffer: &mut String) -> Vec<String> {
    let mut lines = Vec::new();
    while let Some(newline) = buffer.find('\n') {
        let line: String = buffer.drain(..=newline).collect();
        let line = line.trim_end();
        if let Some(data) = line.strip_prefix("data:") {
            lines.push(data.trim().to_string());
        }
    }
    lines
}

// --- OpenAI chunks → Anthropic events ---------------------------------------

/// Turn chat-completions deltas into Anthropic message events.
pub struct OpenAiToAnthropic {
    model: String,
    id: String,
    buffer: String,
    started: bool,
    /// Index of the Anthropic content block currently open.
    block: Option<i64>,
    /// The `tool_calls[].index` that block represents.
    tool: Option<u32>,
    input_tokens: i64,
    output_tokens: i64,
    stop_reason: Option<&'static str>,
    finished: bool,
}

impl OpenAiToAnthropic {
    pub fn new(model: &str, id: &str) -> Self {
        Self {
            model: model.to_string(),
            id: id.to_string(),
            buffer: String::new(),
            started: false,
            block: None,
            tool: None,
            input_tokens: 0,
            output_tokens: 0,
            stop_reason: None,
            finished: false,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.buffer.push_str(&String::from_utf8_lossy(chunk));
        let mut out = String::new();
        for line in drain(&mut self.buffer) {
            if line == "[DONE]" {
                out.push_str(&self.finish());
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if let Some(message) = value.get("error") {
                // A mid-stream failure is reported in the target's shape too.
                out.push_str(&frame(Some("error"), &json!({"type": "error", "error": message})));
                out.push_str(&self.finish());
                continue;
            }
            out.push_str(&self.chunk(&value));
        }
        out.into_bytes()
    }

    fn chunk(&mut self, value: &Value) -> String {
        let mut out = String::new();
        if !self.started {
            self.started = true;
            self.input_tokens = value["usage"]["prompt_tokens"].as_i64().unwrap_or(0);
            out.push_str(&frame(
                Some("message_start"),
                &json!({
                    "type": "message_start",
                    "message": {
                        "id": self.id,
                        "type": "message",
                        "role": "assistant",
                        "model": self.model,
                        "content": [],
                        "stop_reason": null,
                        "stop_sequence": null,
                        "usage": {"input_tokens": self.input_tokens, "output_tokens": 0},
                    },
                }),
            ));
        }
        if let Some(input) = value["usage"]["prompt_tokens"].as_i64() {
            self.input_tokens = input;
        }
        if let Some(output) = value["usage"]["completion_tokens"].as_i64() {
            self.output_tokens = output;
        }

        let Some(choice) = value["choices"].as_array().and_then(|choices| choices.first()) else {
            return out;
        };

        if let Some(text) = choice["delta"]["content"].as_str().filter(|text| !text.is_empty()) {
            out.push_str(&self.open_text_block());
            out.push_str(&frame(
                Some("content_block_delta"),
                &json!({
                    "type": "content_block_delta",
                    "index": self.block,
                    "delta": {"type": "text_delta", "text": text},
                }),
            ));
        }

        if let Some(calls) = choice["delta"]["tool_calls"].as_array() {
            for call in calls {
                let index = call["index"].as_u64().unwrap_or(0) as u32;
                if self.tool != Some(index) {
                    out.push_str(&self.close_block());
                    let block = self.block.map_or(0, |block| block + 1);
                    self.block = Some(block);
                    self.tool = Some(index);
                    out.push_str(&frame(
                        Some("content_block_start"),
                        &json!({
                            "type": "content_block_start",
                            "index": block,
                            "content_block": {
                                "type": "tool_use",
                                "id": call["id"].as_str().unwrap_or_default(),
                                "name": call["function"]["name"].as_str().unwrap_or_default(),
                                "input": {},
                            },
                        }),
                    ));
                }
                if let Some(arguments) = call["function"]["arguments"].as_str() {
                    if !arguments.is_empty() {
                        out.push_str(&frame(
                            Some("content_block_delta"),
                            &json!({
                                "type": "content_block_delta",
                                "index": self.block,
                                "delta": {"type": "input_json_delta", "partial_json": arguments},
                            }),
                        ));
                    }
                }
            }
        }

        if let Some(reason) = choice["finish_reason"].as_str() {
            self.stop_reason = Some(stop_reason_to_anthropic(Some(reason)));
        }
        out
    }

    fn open_text_block(&mut self) -> String {
        if self.block.is_some() && self.tool.is_none() {
            return String::new();
        }
        let mut out = self.close_block();
        let block = self.block.map_or(0, |block| block + 1);
        self.block = Some(block);
        self.tool = None;
        out.push_str(&frame(
            Some("content_block_start"),
            &json!({
                "type": "content_block_start",
                "index": block,
                "content_block": {"type": "text", "text": ""},
            }),
        ));
        out
    }

    fn close_block(&mut self) -> String {
        match self.block.take() {
            Some(index) => frame(
                Some("content_block_stop"),
                &json!({"type": "content_block_stop", "index": index}),
            ),
            None => String::new(),
        }
    }

    /// Close the message. Idempotent: a repeated `[DONE]` adds nothing.
    pub fn finish(&mut self) -> String {
        if self.finished {
            return String::new();
        }
        self.finished = true;
        let mut out = self.close_block();
        let stop_reason = self.stop_reason.unwrap_or("end_turn");
        // OpenAI reports usage at the end, so the input count is only known
        // here; native Anthropic streams carry it in `message_start` instead.
        out.push_str(&frame(
            Some("message_delta"),
            &json!({
                "type": "message_delta",
                "delta": {"stop_reason": stop_reason, "stop_sequence": null},
                "usage": {"input_tokens": self.input_tokens, "output_tokens": self.output_tokens},
            }),
        ));
        out.push_str(&frame(Some("message_stop"), &json!({"type": "message_stop"})));
        out
    }
}

// --- Anthropic events → OpenAI chunks ---------------------------------------

/// Turn Anthropic message events into chat-completions deltas.
pub struct AnthropicToOpenAi {
    model: String,
    id: String,
    buffer: String,
    buffer_event: Option<String>,
    started: bool,
    /// Anthropic block index → OpenAI tool_calls index.
    tools: std::collections::BTreeMap<i64, u32>,
    stop_reason: Option<&'static str>,
    input_tokens: i64,
    output_tokens: i64,
    finished: bool,
}

impl AnthropicToOpenAi {
    pub fn new(model: &str, id: &str) -> Self {
        Self {
            model: model.to_string(),
            id: id.to_string(),
            buffer: String::new(),
            buffer_event: None,
            started: false,
            tools: Default::default(),
            stop_reason: None,
            input_tokens: 0,
            output_tokens: 0,
            finished: false,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.buffer.push_str(&String::from_utf8_lossy(chunk));
        let mut out = String::new();
        // Anthropic frames carry `event:` and `data:`; remember the event name.
        while let Some(newline) = self.buffer.find('\n') {
            let line: String = self.buffer.drain(..=newline).collect();
            let line = line.trim_end().to_string();
            if line.is_empty() {
                self.buffer_event = None;
                continue;
            }
            if let Some(event) = line.strip_prefix("event:") {
                self.buffer_event = Some(event.trim().to_string());
                continue;
            }
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(data.trim()) else {
                continue;
            };
            let event = self.buffer_event.clone().or_else(|| {
                value["type"].as_str().map(str::to_string)
            });
            out.push_str(&self.event(event.as_deref(), &value));
        }
        out.into_bytes()
    }

    fn event(&mut self, event: Option<&str>, value: &Value) -> String {
        let mut out = String::new();
        match event {
            Some("message_start") => {
                self.started = true;
                self.input_tokens = value["message"]["usage"]["input_tokens"].as_i64().unwrap_or(0);
                self.output_tokens = value["message"]["usage"]["output_tokens"].as_i64().unwrap_or(0);
                out.push_str(&self.chunk(json!({
                    "delta": {"role": "assistant", "content": ""},
                    "finish_reason": null,
                })));
            }
            Some("content_block_start") => {
                let block = &value["content_block"];
                if block["type"] == "tool_use" {
                    let index = value["index"].as_i64().unwrap_or(0);
                    let openai_index = self.tools.len() as u32;
                    self.tools.insert(index, openai_index);
                    out.push_str(&self.chunk(json!({
                        "delta": {"tool_calls": [{
                            "index": openai_index,
                            "id": block["id"],
                            "type": "function",
                            "function": {"name": block["name"], "arguments": ""},
                        }]},
                        "finish_reason": null,
                    })));
                }
            }
            Some("content_block_delta") => {
                let delta = &value["delta"];
                if let Some(text) = delta["text"].as_str() {
                    out.push_str(&self.chunk(json!({
                        "delta": {"content": text},
                        "finish_reason": null,
                    })));
                } else if let Some(partial) = delta["partial_json"].as_str() {
                    let block = value["index"].as_i64().unwrap_or(0);
                    let openai_index = self.tools.get(&block).copied().unwrap_or(0);
                    out.push_str(&self.chunk(json!({
                        "delta": {"tool_calls": [{
                            "index": openai_index,
                            "function": {"arguments": partial},
                        }]},
                        "finish_reason": null,
                    })));
                }
            }
            Some("message_delta") => {
                self.stop_reason = Some(finish_reason_to_openai(value["delta"]["stop_reason"].as_str()));
                if let Some(output) = value["usage"]["output_tokens"].as_i64() {
                    self.output_tokens = output;
                }
                out.push_str(&self.chunk(json!({
                    "delta": {},
                    "finish_reason": self.stop_reason.unwrap_or("stop"),
                })));
            }
            Some("message_stop") => out.push_str(&self.finish()),
            Some("error") => {
                out.push_str(&format!("data: {}\n\n", json!({"error": value["error"]})));
                out.push_str(&self.finish());
            }
            _ => {}
        }
        out
    }

    fn chunk(&self, choice: Value) -> String {
        let mut choice = choice;
        choice["index"] = json!(0);
        format!(
            "data: {}\n\n",
            json!({
                "id": self.id,
                "object": "chat.completion.chunk",
                "created": 0,
                "model": self.model,
                "choices": [choice],
                "usage": if choice.get("finish_reason").is_some_and(|r| !r.is_null()) {
                    json!({
                        "prompt_tokens": self.input_tokens,
                        "completion_tokens": self.output_tokens,
                        "total_tokens": self.input_tokens + self.output_tokens,
                    })
                } else {
                    Value::Null
                },
            })
        )
    }

    pub fn finish(&mut self) -> String {
        if self.finished {
            return String::new();
        }
        self.finished = true;
        "data: [DONE]\n\n".to_string()
    }
}

/// Adapt an upstream byte stream into the client's SSE shape (ITEM-094 AC4).
///
/// `provider` is the surface the upstream speaks; the client speaks the other
/// one. The translator sits **before** the telemetry tap, so usage parsing and
/// completion capture read the same frames the client does.
pub(crate) fn adapt<S>(
    stream: S,
    provider: super::Surface,
    model: String,
    id: String,
) -> impl futures_util::Stream<Item = Result<axum::body::Bytes, reqwest::Error>>
where
    S: futures_util::Stream<Item = Result<axum::body::Bytes, reqwest::Error>>,
{
    use futures_util::StreamExt;
    async_stream::stream! {
        let mut stream = std::pin::pin!(stream);
        let mut openai_to_anthropic = OpenAiToAnthropic::new(&model, &id);
        let mut anthropic_to_openai = AnthropicToOpenAi::new(&model, &id);
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    let out = match provider {
                        super::Surface::OpenAi => openai_to_anthropic.push(&bytes),
                        super::Surface::Anthropic => anthropic_to_openai.push(&bytes),
                    };
                    if !out.is_empty() {
                        yield Ok(axum::body::Bytes::from(out));
                    }
                }
                Err(e) => {
                    tracing::warn!("translated stream ended early: {e}");
                    return;
                }
            }
        }
        let tail = match provider {
            super::Surface::OpenAi => openai_to_anthropic.finish(),
            super::Surface::Anthropic => anthropic_to_openai.finish(),
        };
        if !tail.is_empty() {
            yield Ok(axum::body::Bytes::from(tail.into_bytes()));
        }
    }
}

#[cfg(test)]
mod tests;
