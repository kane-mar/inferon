// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! Cross-surface translation (ITEM-094).
//!
//! An Anthropic SDK should be able to talk to a DeepSeek/OpenAI-compatible
//! backend, and an OpenAI SDK to Claude. Both shapes are translated here, as
//! pure functions: the proxy decides *whether* to translate, this module only
//! maps one payload to the other.
//!
//! The rule is never to lose information silently. A block the other surface
//! cannot represent (an image, a document) returns [`Unsupported`] naming the
//! field, so the caller can answer a typed 400 instead of quietly changing the
//! question.

use serde_json::{json, Value};

/// The surfaces Inferon speaks (the proxy's own enum, re-used here so the
/// translation seam cannot drift from the routes).
pub(crate) use crate::proxy::Surface;

/// A request (or response) field the target surface cannot represent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsupported {
    /// Where it is, e.g. `messages[2].content[0].image`.
    pub field: String,
    /// What to tell the caller.
    pub detail: String,
}

impl Unsupported {
    fn new(field: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            detail: detail.into(),
        }
    }
}

/// Anthropic default when a translated request omits `max_tokens` (the field is
/// required there, optional in chat completions).
const DEFAULT_MAX_TOKENS: i64 = 4096;

// --- Anthropic → OpenAI -----------------------------------------------------

/// Translate an Anthropic Messages request into a chat completions request.
pub fn request_to_openai(request: &Value) -> Result<Value, Unsupported> {
    let mut messages: Vec<Value> = Vec::new();

    // The Anthropic `system` is top-level: a string or an array of blocks.
    if let Some(system) = request.get("system") {
        let text = match system {
            Value::String(text) => text.clone(),
            Value::Array(blocks) => text_of_blocks(blocks, "system")?,
            Value::Null => String::new(),
            _ => return Err(Unsupported::new("system", "unsupported system shape")),
        };
        if !text.is_empty() {
            messages.push(json!({"role": "system", "content": text}));
        }
    }

    let turns = request["messages"]
        .as_array()
        .ok_or_else(|| Unsupported::new("messages", "messages must be an array"))?;
    for (index, turn) in turns.iter().enumerate() {
        let role = turn["role"]
            .as_str()
            .ok_or_else(|| Unsupported::new(format!("messages[{index}].role"), "missing role"))?;
        let content = &turn["content"];
        let text = match content {
            Value::String(text) => text.clone(),
            Value::Array(blocks) => {
                // Tool calls and results become their own OpenAI messages.
                let mut plain = Vec::new();
                let mut tool_calls = Vec::new();
                let mut tool_results = Vec::new();
                for (block_index, block) in blocks.iter().enumerate() {
                    let at = format!("messages[{index}].content[{block_index}]");
                    match block["type"].as_str().unwrap_or_default() {
                        "text" => plain.push(block["text"].as_str().unwrap_or_default().to_string()),
                        "tool_use" => tool_calls.push(json!({
                            "id": block["id"],
                            "type": "function",
                            "function": {
                                "name": block["name"],
                                "arguments": block["input"].to_string(),
                            },
                        })),
                        "tool_result" => tool_results.push(json!({
                            "role": "tool",
                            "tool_call_id": block["tool_use_id"],
                            "content": tool_result_text(&block["content"]),
                        })),
                        "image" | "document" => {
                            return Err(Unsupported::new(
                                format!("{at}.{}", block["type"].as_str().unwrap_or("block")),
                                "the target surface accepts text only",
                            ))
                        }
                        other => {
                            return Err(Unsupported::new(
                                format!("{at}.{other}"),
                                "unsupported content block",
                            ))
                        }
                    }
                }
                // Tool results are user-visible turns of their own.
                messages.extend(tool_results);
                if !tool_calls.is_empty() {
                    messages.push(json!({
                        "role": "assistant",
                        "content": if plain.is_empty() { Value::Null } else { json!(plain.join("\n")) },
                        "tool_calls": tool_calls,
                    }));
                    continue;
                }
                plain.join("\n")
            }
            _ => String::new(),
        };
        if !text.is_empty() || tool_results_empty(content) {
            messages.push(json!({"role": role, "content": text}));
        }
    }

    let mut translated = json!({
        "model": request["model"],
        "messages": messages,
    });
    copy(request, &mut translated, "temperature");
    copy(request, &mut translated, "top_p");
    if let Some(max) = request.get("max_tokens") {
        translated["max_tokens"] = max.clone();
    }
    if let Some(stop) = request.get("stop_sequences") {
        translated["stop"] = stop.clone();
    }
    if let Some(tools) = request["tools"].as_array() {
        translated["tools"] = Value::Array(
            tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool["name"],
                            "description": tool["description"],
                            "parameters": tool["input_schema"],
                        },
                    })
                })
                .collect(),
        );
    }
    if let Some(choice) = request.get("tool_choice") {
        translated["tool_choice"] = match choice["type"].as_str() {
            Some("auto") => json!("auto"),
            Some("any") => json!("required"),
            Some("none") => json!("none"),
            Some("tool") => json!({"type": "function", "function": {"name": choice["name"]}}),
            _ => choice.clone(),
        };
    }
    if request["stream"].as_bool() == Some(true) {
        translated["stream"] = json!(true);
    }
    Ok(translated)
}

fn tool_results_empty(content: &Value) -> bool {
    // A turn that was only tool results has already been emitted (or was empty).
    content.as_array().is_none_or(|blocks| {
        !blocks
            .iter()
            .any(|block| block["type"].as_str() == Some("tool_result"))
    })
}

fn text_of_blocks(blocks: &[Value], at: &str) -> Result<String, Unsupported> {
    let mut parts = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        match block["type"].as_str().unwrap_or_default() {
            "text" => parts.push(block["text"].as_str().unwrap_or_default().to_string()),
            other => {
                return Err(Unsupported::new(
                    format!("{at}[{index}].{other}"),
                    "the target surface accepts text only",
                ))
            }
        }
    }
    Ok(parts.join("\n"))
}

/// Tool results may be a string or text blocks.
fn tool_result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Translate a chat completions response into an Anthropic message.
pub fn response_to_anthropic(response: &Value) -> Result<Value, Unsupported> {
    let choice = response["choices"]
        .as_array()
        .and_then(|choices| choices.first())
        .ok_or_else(|| Unsupported::new("choices", "the upstream returned no choices"))?;
    let message = &choice["message"];

    let mut content: Vec<Value> = Vec::new();
    if let Some(text) = message["content"].as_str().filter(|text| !text.is_empty()) {
        content.push(json!({"type": "text", "text": text}));
    }
    if let Some(calls) = message["tool_calls"].as_array() {
        for call in calls {
            content.push(json!({
                "type": "tool_use",
                "id": call["id"],
                "name": call["function"]["name"],
                "input": arguments_value(&call["function"]["arguments"]),
            }));
        }
    }

    Ok(json!({
        "id": response["id"],
        "type": "message",
        "role": "assistant",
        "model": response["model"],
        "content": content,
        "stop_reason": stop_reason_to_anthropic(choice["finish_reason"].as_str()),
        "stop_sequence": null,
        "usage": {
            "input_tokens": response["usage"]["prompt_tokens"].as_i64().unwrap_or(0),
            "output_tokens": response["usage"]["completion_tokens"].as_i64().unwrap_or(0),
        },
    }))
}

/// Tool arguments arrive as a JSON string; a malformed one is passed through as
/// `{}` rather than failing an otherwise good answer.
fn arguments_value(arguments: &Value) -> Value {
    match arguments {
        Value::String(text) => serde_json::from_str(text).unwrap_or_else(|_| json!({})),
        Value::Object(_) => arguments.clone(),
        _ => json!({}),
    }
}

pub fn stop_reason_to_anthropic(finish_reason: Option<&str>) -> &'static str {
    match finish_reason {
        Some("length") => "max_tokens",
        Some("tool_calls") | Some("function_call") => "tool_use",
        // `content_filter` has no Anthropic equivalent; the turn still ended.
        _ => "end_turn",
    }
}

// --- OpenAI → Anthropic -----------------------------------------------------

/// Translate a chat completions request into an Anthropic Messages request.
pub fn request_to_anthropic(request: &Value) -> Result<Value, Unsupported> {
    let mut system: Vec<String> = Vec::new();
    let mut messages: Vec<Value> = Vec::new();

    let turns = request["messages"]
        .as_array()
        .ok_or_else(|| Unsupported::new("messages", "messages must be an array"))?;
    for (index, turn) in turns.iter().enumerate() {
        let role = turn["role"]
            .as_str()
            .ok_or_else(|| Unsupported::new(format!("messages[{index}].role"), "missing role"))?;
        match role {
            "system" | "developer" => system.push(content_text(&turn["content"])),
            "tool" => {
                // Consecutive tool results belong to one Anthropic user turn.
                let block = json!({
                    "type": "tool_result",
                    "tool_use_id": turn["tool_call_id"],
                    "content": content_text(&turn["content"]),
                });
                match messages.last_mut() {
                    Some(last)
                        if last["role"] == "user"
                            && last["content"].as_array().is_some_and(|blocks| {
                                blocks.iter().all(|b| b["type"] == "tool_result")
                            }) =>
                    {
                        last["content"].as_array_mut().unwrap().push(block);
                    }
                    _ => messages.push(json!({"role": "user", "content": [block]})),
                }
            }
            "assistant" => {
                let mut blocks = Vec::new();
                let text = content_text(&turn["content"]);
                if !text.is_empty() {
                    blocks.push(json!({"type": "text", "text": text}));
                }
                if let Some(calls) = turn["tool_calls"].as_array() {
                    for call in calls {
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": call["id"],
                            "name": call["function"]["name"],
                            "input": arguments_value(&call["function"]["arguments"]),
                        }));
                    }
                }
                messages.push(json!({
                    "role": "assistant",
                    "content": if blocks.is_empty() { json!("") } else { json!(blocks) },
                }));
            }
            "user" => {
                if let Some(parts) = turn["content"].as_array() {
                    let mut blocks = Vec::new();
                    for (part_index, part) in parts.iter().enumerate() {
                        match part["type"].as_str().unwrap_or_default() {
                            "text" => blocks.push(json!({"type": "text", "text": part["text"]})),
                            other => {
                                return Err(Unsupported::new(
                                    format!("messages[{index}].content[{part_index}].{other}"),
                                    "the target surface accepts text only",
                                ))
                            }
                        }
                    }
                    messages.push(json!({"role": "user", "content": blocks}));
                } else {
                    messages.push(json!({"role": "user", "content": content_text(&turn["content"])}));
                }
            }
            other => {
                return Err(Unsupported::new(
                    format!("messages[{index}].{other}"),
                    "unsupported message role",
                ))
            }
        }
    }

    let mut translated = json!({
        "model": request["model"],
        "messages": messages,
        "max_tokens": request["max_tokens"].as_i64().unwrap_or(DEFAULT_MAX_TOKENS),
    });
    if !system.is_empty() {
        translated["system"] = json!(system.join("\n\n"));
    }
    copy(request, &mut translated, "temperature");
    copy(request, &mut translated, "top_p");
    if let Some(stop) = request.get("stop") {
        translated["stop_sequences"] = stop.clone();
    }
    if let Some(tools) = request["tools"].as_array() {
        let translated_tools: Vec<Value> = tools
            .iter()
            .filter_map(|tool| {
                tool["function"].as_object().map(|function| {
                    json!({
                        "name": function.get("name"),
                        "description": function.get("description"),
                        "input_schema": function
                            .get("parameters")
                            .cloned()
                            .unwrap_or_else(|| json!({"type": "object"})),
                    })
                })
            })
            .collect();
        if !translated_tools.is_empty() {
            translated["tools"] = Value::Array(translated_tools);
        }
    }
    if let Some(choice) = request.get("tool_choice") {
        translated["tool_choice"] = match choice {
            Value::String(name) => match name.as_str() {
                "auto" => json!({"type": "auto"}),
                "none" => json!({"type": "none"}),
                "required" => json!({"type": "any"}),
                _ => json!({"type": "auto"}),
            },
            Value::Object(_) => json!({
                "type": "tool",
                "name": choice["function"]["name"],
            }),
            _ => json!({"type": "auto"}),
        };
    }
    if request["stream"].as_bool() == Some(true) {
        translated["stream"] = json!(true);
    }
    Ok(translated)
}

/// Translate an Anthropic message into a chat completions response.
pub fn response_to_openai(response: &Value) -> Result<Value, Unsupported> {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    for (index, block) in response["content"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .enumerate()
    {
        match block["type"].as_str().unwrap_or_default() {
            "text" => text.push_str(block["text"].as_str().unwrap_or_default()),
            "tool_use" => tool_calls.push(json!({
                "id": block["id"],
                "type": "function",
                "index": index,
                "function": {
                    "name": block["name"],
                    "arguments": block["input"].to_string(),
                },
            })),
            "thinking" | "redacted_thinking" => {}
            other => {
                return Err(Unsupported::new(
                    format!("content[{index}].{other}"),
                    "unsupported content block in the upstream response",
                ))
            }
        }
    }

    let mut message = json!({
        "role": "assistant",
        "content": if text.is_empty() { Value::Null } else { json!(text) },
    });
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
    }

    Ok(json!({
        "id": response["id"],
        "object": "chat.completion",
        "created": response["created"].as_i64().unwrap_or(0),
        "model": response["model"],
        "choices": [{
            "index": 0,
            "message": message,
            "logprobs": null,
            "finish_reason": finish_reason_to_openai(response["stop_reason"].as_str()),
        }],
        "usage": {
            "prompt_tokens": response["usage"]["input_tokens"].as_i64().unwrap_or(0),
            "completion_tokens": response["usage"]["output_tokens"].as_i64().unwrap_or(0),
            "total_tokens": response["usage"]["input_tokens"].as_i64().unwrap_or(0)
                + response["usage"]["output_tokens"].as_i64().unwrap_or(0),
        },
    }))
}

/// Translate an upstream error body into the client's shape, keeping the
/// provider's message (the caller still sees why the call failed).
pub fn error_to_anthropic(error: &Value) -> Value {
    let detail = &error["error"];
    json!({
        "type": "error",
        "error": {
            "type": detail["type"].as_str().unwrap_or("api_error"),
            "message": detail["message"].as_str().unwrap_or("upstream request failed"),
        },
    })
}

pub fn error_to_openai(error: &Value) -> Value {
    let detail = &error["error"];
    json!({
        "error": {
            "message": detail["message"].as_str().unwrap_or("upstream request failed"),
            "type": detail["type"].as_str().unwrap_or("api_error"),
        },
    })
}

pub fn finish_reason_to_openai(stop_reason: Option<&str>) -> &'static str {
    match stop_reason {
        Some("max_tokens") => "length",
        Some("tool_use") => "tool_calls",
        _ => "stop",
    }
}

/// Text of an OpenAI `content`, which is a string or an array of text parts.
fn content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn copy(from: &Value, to: &mut Value, field: &str) {
    if let Some(value) = from.get(field) {
        if !value.is_null() {
            to[field] = value.clone();
        }
    }
}

pub mod sse;

#[cfg(test)]
mod tests;
