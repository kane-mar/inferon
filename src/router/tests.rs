    use super::*;

    fn payload(model: &str, body: Value) -> Value {
        let mut p = serde_json::json!({"model": model, "messages": [{"role": "user", "content": "hi"}]});
        if let Some(obj) = body.as_object() {
            for (k, v) in obj {
                p[k] = v.clone();
            }
        }
        p
    }

    #[test]
    fn plain_short_chat_is_simple() {
        let p = payload("gpt-4o", serde_json::json!({}));
        assert!(complexity_score(&p) < DOWNGRADE_THRESHOLD);
        assert_eq!(maybe_downgrade(&p).as_deref(), Some("gpt-4o-mini"));
    }

    #[test]
    fn tools_make_it_complex() {
        let p = payload(
            "gpt-4o",
            serde_json::json!({"tools": [{"type": "function", "function": {"name": "f"}}]}),
        );
        assert_eq!(complexity_score(&p), 0.50);
        assert_eq!(maybe_downgrade(&p), None);
    }

    #[test]
    fn json_mode_alone_scores_below_threshold() {
        // Spec math: response_format adds +0.30, still < 0.35 -> downgraded.
        let p = payload(
            "gpt-4o",
            serde_json::json!({"response_format": {"type": "json_object"}}),
        );
        assert_eq!(complexity_score(&p), 0.30);
        assert_eq!(maybe_downgrade(&p).as_deref(), Some("gpt-4o-mini"));
    }

    #[test]
    fn json_mode_plus_long_content_exceeds_threshold() {
        let p = payload(
            "gpt-4o",
            serde_json::json!({"response_format": {"type": "json_object"}}),
        );
        let mut p = p;
        p["messages"] = serde_json::json!([{"role": "user", "content": "x".repeat(4000)}]);
        assert_eq!(complexity_score(&p), 0.50);
        assert_eq!(maybe_downgrade(&p), None);
    }

    #[test]
    fn long_content_scores_below_threshold() {
        // 4000 chars adds +0.20 only -> still simple by spec math.
        let mut p = payload("gpt-4o", serde_json::json!({}));
        p["messages"] = serde_json::json!([{"role": "user", "content": "x".repeat(4000)}]);
        assert_eq!(complexity_score(&p), 0.20);
        assert_eq!(maybe_downgrade(&p).as_deref(), Some("gpt-4o-mini"));
    }

    #[test]
    fn code_or_json_spec_plus_length_exceeds_threshold() {
        // Code markers +0.30 alone stays below; combined with >3000 chars it
        // clears the bar (0.30 + 0.20 = 0.50).
        let mut code = payload("gpt-4o", serde_json::json!({}));
        code["messages"] = serde_json::json!([{"role": "user", "content": "fn main() {\n  import x\n}"}]);
        assert_eq!(complexity_score(&code), 0.30);
        code["messages"] = serde_json::json!([{"role": "user", "content": "fn main() {\n  import x\n}".repeat(200)}]);
        assert_eq!(complexity_score(&code), 0.50);
        assert_eq!(maybe_downgrade(&code), None);
    }

    #[test]
    fn code_and_json_specs_score_higher() {
        let code = payload("gpt-4o", serde_json::json!({}));
        let mut code = code;
        code["messages"] =
            serde_json::json!([{"role": "user", "content": "fn main() {\n  import x\n}"}]);
        assert_eq!(complexity_score(&code), 0.30);

        let spec = payload("gpt-4o", serde_json::json!({}));
        let mut spec = spec;
        spec["messages"] =
            serde_json::json!([{"role": "user", "content": "{\"type\": \"object\", \"properties\": {}}"}]);
        assert_eq!(complexity_score(&spec), 0.25);
    }

    #[test]
    fn score_is_capped_at_one() {
        let mut p = payload("gpt-4o", serde_json::json!({}));
        p["tools"] = serde_json::json!([{"type": "function"}]);
        p["response_format"] = serde_json::json!({"type": "json_object"});
        p["messages"] = serde_json::json!([{"role": "user", "content": "y".repeat(9000)}]);
        assert_eq!(complexity_score(&p), 1.0);
    }

    #[test]
    fn never_downgrades_mini_or_other_models() {
        assert_eq!(maybe_downgrade(&payload("gpt-4o-mini", serde_json::json!({}))), None);
        assert_eq!(maybe_downgrade(&payload("deepseek-chat", serde_json::json!({}))), None);
    }

    #[test]
    fn the_threshold_is_configurable_per_tenant() {
        // A trivial payload scores 0.0 — below any positive threshold, never
        // below 0.0 (ITEM-046 turns the router off by making it unreachable).
        let simple = payload("gpt-4o", serde_json::json!({}));
        assert_eq!(maybe_downgrade_with(&simple, 0.0), None);
        assert_eq!(maybe_downgrade_with(&simple, 0.35), Some("gpt-4o-mini".to_string()));
    }

    #[test]
    fn deepseek_tier_is_downgraded_by_the_same_router() {
        // ITEM-054: the pair table includes a DeepSeek pair, so the router is no
        // longer gpt-4o-only.
        let simple = payload("deepseek-v4-pro", serde_json::json!({}));
        assert_eq!(maybe_downgrade(&simple).as_deref(), Some("deepseek-flash"));
        // The cheap member is never downgraded again.
        assert_eq!(maybe_downgrade(&payload("deepseek-flash", serde_json::json!({}))), None);
        // `deepseek-chat` is not a pair member.
        assert_eq!(maybe_downgrade(&payload("deepseek-chat", serde_json::json!({}))), None);
        // Target mapping.
        assert_eq!(downgrade_target("deepseek-v4-pro"), Some("deepseek-flash"));
        assert_eq!(downgrade_target("gpt-4o"), Some("gpt-4o-mini"));
        assert_eq!(downgrade_target("gpt-4o-mini"), None);
        assert_eq!(downgrade_target("deepseek-flash"), None);
    }
