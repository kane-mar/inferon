//! Cost anti-pattern detection (ITEM-006, AC1–AC3/AC5). Runs in the
//! background batch worker, never on the hot path.
//!
//! Rules (source: backlog ACs + slice spec waste rules):
//!   A. a **frontier model with a cheaper tier pair** (any `router::TIER_PAIRS`
//!      member) handling <100 prompt and <50 completion tokens ->
//!      over-provisioned; suggest the paired cheap model.
//!   B. A system prompt that repeats (>=2 occurrences) at >=4000 prompt
//!      tokens with no provider cache benefit (cached_tokens == 0) ->
//!      cache-miss system prompt.
//!   C. a frontier model used for simple unstructured text (no tools / functions
//!      / response_format, short plain-string messages) -> model misuse.
//!
//! A and C follow the router's configured pairs, so they cover DeepSeek traffic
//! (`deepseek-v4-pro` -> `deepseek-flash`) as well as the original gpt-4o tier.

use crate::TraceMessage;
use serde_json::{json, Value};

/// Flag count thresholds for rule B (prompt tokens) and rule C (content chars).
const RULE_B_MIN_PROMPT_TOKENS: i32 = 4000;
const RULE_C_MAX_CONTENT_CHARS: usize = 3000;

/// Simple unstructured text: no tools/functions/response_format and every
/// message is a plain string under the length cap.
fn is_simple_text(raw_prompt: &Option<Value>) -> bool {
    let Some(prompt) = raw_prompt else {
        return false;
    };
    for key in ["tools", "functions", "response_format"] {
        if prompt.get(key).is_some_and(|v| !v.is_null()) {
            return false;
        }
    }
    let mut chars = 0usize;
    for message in prompt["messages"].as_array().into_iter().flatten() {
        match message["content"].as_str() {
            Some(text) => chars += text.len(),
            // Multimodal / non-string content is not "unstructured text".
            None => return false,
        }
    }
    chars <= RULE_C_MAX_CONTENT_CHARS
}

/// Lint one trace. `repeat_count` is how many times this system_prompt_hash
/// has been seen so far (including this trace), maintained by the worker.
/// Returns `Some(waste_detected)` JSON when at least one rule fires.
pub fn lint(msg: &TraceMessage, repeat_count: u32) -> Option<Value> {
    let mut waste = json!({});
    let mut fired = false;

    // Rules A/C: a frontier model used for trivial work, where the router's
    // configured pair names the cheaper model to suggest. The router normally
    // downsizes these before serving, so this fires when the router is off or
    // the model is unpaired.
    if let Some(suggested) = crate::router::downgrade_target(&msg.request_model) {
        if msg.prompt_tokens < 100 && msg.completion_tokens < 50 {
            waste["over_provisioned"] = json!(true);
            waste["suggested_model"] = json!(suggested);
            fired = true;
        }
        if is_simple_text(&msg.raw_prompt) {
            waste["model_misuse"] = json!(true);
            waste["suggested_model"] = json!(suggested);
            fired = true;
        }
    }

    if repeat_count >= 2
        && msg.system_prompt_hash.is_some()
        && msg.prompt_tokens >= RULE_B_MIN_PROMPT_TOKENS
        && msg.cached_prompt_tokens == Some(0)
    {
        waste["cache_miss_system_prompt"] = json!(true);
        waste["prompt_tokens"] = json!(msg.prompt_tokens);
        fired = true;
    }

    fired.then_some(waste)
}

#[cfg(test)]
mod tests;
