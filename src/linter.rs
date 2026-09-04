//! Cost anti-pattern detection (ITEM-006, AC1–AC3/AC5). Runs in the
//! background batch worker, never on the hot path.
//!
//! Rules (source: backlog ACs + slice spec waste rules):
//!   A. gpt-4o-tier request with <100 prompt and <50 completion tokens ->
//!      over-provisioned; suggest gpt-4o-mini.
//!   B. A system prompt that repeats (>=2 occurrences) at >=4000 prompt
//!      tokens with no provider cache benefit (cached_tokens == 0) ->
//!      cache-miss system prompt.
//!   C. gpt-4o-tier used for simple unstructured text (no tools / functions /
//!      response_format, short plain-string messages) -> model misuse.

use crate::TraceMessage;
use serde_json::{json, Value};

/// Flag count thresholds for rule B (prompt tokens) and rule C (content chars).
const RULE_B_MIN_PROMPT_TOKENS: i32 = 4000;
const RULE_C_MAX_CONTENT_CHARS: usize = 3000;
const MINI: &str = "gpt-4o-mini";

/// gpt-4o tier but NOT gpt-4o-mini (a mini request can't be over-provisioned
/// against mini). Prefix-tolerant for dated snapshots.
fn is_gpt4o_full(model: &str) -> bool {
    model.starts_with("gpt-4o") && !model.starts_with("gpt-4o-mini")
}

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

    if is_gpt4o_full(&msg.request_model) {
        if msg.prompt_tokens < 100 && msg.completion_tokens < 50 {
            waste["over_provisioned"] = json!(true);
            waste["suggested_model"] = json!(MINI);
            fired = true;
        }
        if is_simple_text(&msg.raw_prompt) {
            waste["model_misuse"] = json!(true);
            waste["suggested_model"] = json!(MINI);
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
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use uuid::Uuid;

    fn trace(model: &str, prompt: i32, completion: i32) -> TraceMessage {
        TraceMessage {
            id: Uuid::new_v4(),
            tenant_id: Uuid::new_v4(),
            request_model: model.to_string(),
            system_prompt_hash: None,
            prompt_tokens: prompt,
            completion_tokens: completion,
            cached_prompt_tokens: None,
            ttft_ms: Some(1),
            total_duration_ms: 1,
            status_code: 200,
            raw_prompt: None,
            raw_completion: None,
            waste_detected: None,
            upstream_cost_usd: Decimal::ZERO,
        }
    }

    fn with_system_prompt(msg: &mut TraceMessage, hash: &str, prompt_tokens: i32) {
        msg.system_prompt_hash = Some(hash.to_string());
        msg.prompt_tokens = prompt_tokens;
    }

    #[test]
    fn rule_a_flags_small_gpt4o_requests() {
        let waste = lint(&trace("gpt-4o", 80, 20), 1).expect("flagged");
        assert_eq!(waste["over_provisioned"], true);
        assert_eq!(waste["suggested_model"], "gpt-4o-mini");
    }

    #[test]
    fn rule_a_is_prefix_tolerant_and_skips_mini() {
        assert!(lint(&trace("gpt-4o-2024-08-06", 80, 20), 1).is_some());
        assert!(lint(&trace("gpt-4o-mini", 80, 20), 1).is_none(), "mini is not over-provisioned");
        assert!(lint(&trace("deepseek-chat", 80, 20), 1).is_none());
    }

    #[test]
    fn rule_a_ignores_large_requests() {
        assert!(lint(&trace("gpt-4o", 500, 400), 1).is_none());
    }

    #[test]
    fn rule_b_flags_repeated_large_uncached_system_prompts() {
        let mut msg = trace("deepseek-chat", 5000, 100);
        with_system_prompt(&mut msg, "hash-abc", 5000);
        msg.cached_prompt_tokens = Some(0);
        assert!(lint(&msg, 1).is_none(), "first occurrence is not yet repeating");
        let waste = lint(&msg, 2).expect("flagged on repeat");
        assert_eq!(waste["cache_miss_system_prompt"], true);
        assert_eq!(waste["prompt_tokens"], 5000);
    }

    #[test]
    fn rule_b_skips_cached_or_small_or_hashless() {
        let mut cached = trace("deepseek-chat", 5000, 100);
        with_system_prompt(&mut cached, "h", 5000);
        cached.cached_prompt_tokens = Some(1200);
        assert!(lint(&cached, 2).is_none(), "cache benefit observed");

        let mut small = trace("deepseek-chat", 3000, 100);
        with_system_prompt(&mut small, "h", 3000);
        small.cached_prompt_tokens = Some(0);
        assert!(lint(&small, 2).is_none(), "below the 4000-token bar");

        let mut hashless = trace("deepseek-chat", 9000, 100);
        hashless.cached_prompt_tokens = Some(0);
        assert!(lint(&hashless, 9).is_none(), "no system prompt to cache");
    }

    fn simple_prompt() -> Option<Value> {
        Some(json!({"messages": [{"role": "user", "content": "write a haiku"}]}))
    }

    fn tool_prompt() -> Option<Value> {
        Some(json!({"messages": [{"role": "user", "content": "hi"}],
                     "tools": [{"type": "function", "function": {"name": "f"}}]}))
    }

    #[test]
    fn rule_c_flags_gpt4o_on_simple_text() {
        let mut msg = trace("gpt-4o", 300, 100);
        msg.raw_prompt = simple_prompt();
        let waste = lint(&msg, 1).expect("flagged");
        assert_eq!(waste["model_misuse"], true);
        assert_eq!(waste["suggested_model"], "gpt-4o-mini");
    }

    #[test]
    fn rule_c_respects_tools_response_format_and_size() {
        let mut with_tools = trace("gpt-4o", 300, 100);
        with_tools.raw_prompt = tool_prompt();
        assert!(lint(&with_tools, 1).is_none(), "tool use is not simple text");

        let mut json_mode = trace("gpt-4o", 300, 100);
        json_mode.raw_prompt =
            Some(json!({"messages": [{"role": "user", "content": "hi"}], "response_format": {"type": "json_object"}}));
        assert!(lint(&json_mode, 1).is_none());

        let mut long = trace("gpt-4o", 300, 100);
        long.raw_prompt = Some(json!({"messages": [{"role": "user", "content": "x".repeat(4000)}]}));
        assert!(lint(&long, 1).is_none(), "long structured content is not simple");

        let mut multimodal = trace("gpt-4o", 300, 100);
        multimodal.raw_prompt = Some(json!({"messages": [{"role": "user", "content": [{"type": "image_url"}]}]}));
        assert!(lint(&multimodal, 1).is_none());
    }

    #[test]
    fn rule_c_never_fires_without_raw_prompt_or_for_deepseek() {
        assert!(lint(&trace("gpt-4o", 300, 100), 1).is_none(), "no payload to judge");
        let mut deepseek = trace("deepseek-chat", 300, 100);
        deepseek.raw_prompt = simple_prompt();
        assert!(lint(&deepseek, 1).is_none(), "rule C targets gpt-4o tier only");
    }
}
