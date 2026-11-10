// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

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
            raw_completion_capped: false,
            waste_detected: None,
            upstream_cost_usd: Decimal::ZERO,
            cache_savings_usd: Decimal::ZERO,
            truncated: false,
            guard_event: None,
            routing_event: None,
            router_event: None,
            provider_id: None,
            is_local: false,
            api_key_id: None,
            outcome: crate::quality::Outcome::Ok,
            error_class: None,
            savings_usd: rust_decimal::Decimal::ZERO,
            savings_kind: None,
            cache_affinity: None,
            policy_version: None,
            proxy_ms: None,
                proxy_us: None,
        body_ms: None,
        prevented_usd: rust_decimal::Decimal::ZERO,
        prevented_kind: None,
                example_state: None,
                example_reason: None,
                completion_hash: None,
                session_id: None,
                request_lint: None,
                prune_event: None,
                pii: None,
                provider_selection: None,
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

    #[test]
    fn rules_a_and_c_follow_the_router_pairs_not_just_gpt4o() {
        // DeepSeek's pair is configured too, so the same leaks are detected.
        let waste = lint(&trace("deepseek-v4-pro", 80, 20), 1).expect("flagged");
        assert_eq!(waste["over_provisioned"], true);
        assert_eq!(waste["suggested_model"], "deepseek-flash");

        let mut msg = trace("deepseek-v4-pro", 300, 100);
        msg.raw_prompt = simple_prompt();
        let waste = lint(&msg, 1).expect("flagged");
        assert_eq!(waste["model_misuse"], true);
        assert_eq!(waste["suggested_model"], "deepseek-flash");

        // The cheap member is not over-provisioned, and an unpaired model has no
        // suggestion to offer.
        assert!(lint(&trace("deepseek-flash", 80, 20), 1).is_none());
        assert!(lint(&trace("qwen/qwen3.8-27b", 80, 20), 1).is_none());
    }

#[test]
fn a_clean_trace_still_records_that_it_was_checked() {
    // The worker stores `record`, not `lint`: a non-NULL value means "checked".
    // Without this, `waste_detected IS NULL` meant both "clean" and "never ran",
    // so the dashboard could not show that the linter was working at all.
    let mut msg = trace("deepseek-flash", 12, 4);
    msg.raw_prompt = Some(serde_json::json!({"tools": [{"a": 1}], "messages": []}));
    assert!(lint(&msg, 0).is_none(), "nothing fires for a cheap model");
    assert_eq!(record(&msg, 0), serde_json::json!({}), "but the scan is recorded");
}
