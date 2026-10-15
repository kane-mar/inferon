    use super::*;

    #[test]
    fn cache_savings_is_uncached_minus_cached() {
        // deepseek-chat: miss 0.22, hit 0.007 per 1M. 1M prompt, 500k cached:
        // uncached 0.22; cached 0.5*0.22 + 0.5*0.007 = 0.1135 -> saving 0.1065.
        assert_eq!(
            cache_savings("deepseek-chat", 1_000_000, 0, 500_000),
            dec("0.1065")
        );
        // Nothing cached -> no saving.
        assert_eq!(cache_savings("deepseek-chat", 1_000_000, 0, 0), Decimal::ZERO);
        // A miss-only model (no hit rate) -> no saving.
        assert_eq!(cache_savings("gpt-4o", 1_000_000, 0, 500_000), Decimal::ZERO);
    }

    fn mk(miss: &str, completion: &str) -> Price {
        miss_only(miss, completion)
    }
    fn mk_hit(miss: &str, hit_rate: &str, completion: &str) -> Price {
        hit(miss, hit_rate, completion)
    }

    #[test]
    fn prices_match_the_spec_table() {
        assert_eq!(price_for("gpt-4o"), mk("2.50", "10.00"));
        assert_eq!(price_for("gpt-4o-mini"), mk("0.15", "0.60"));
        assert_eq!(price_for("gpt-4o-2024-08-06"), mk("2.50", "10.00"));
        assert_eq!(price_for("gpt-4o-mini-2024-07-18"), mk("0.15", "0.60"));
        assert_eq!(price_for("unknown-model"), mk("3.00", "15.00"));
    }

    #[test]
    fn deepseek_chat_prices_at_v4_flash_rates() {
        // Oracle: DeepSeek pricing page — v4-flash input $0.22 cache-miss /
        // $0.007 cache-hit, output $0.66 off-peak per 1M; deepseek-chat
        // resolves to v4-flash. v4-pro: $0.66 / $0.022 / $1.98.
        assert_eq!(price_for("deepseek-chat"), mk_hit("0.22", "0.007", "0.66"));
        assert_eq!(price_for("deepseek-v4-flash"), mk_hit("0.22", "0.007", "0.66"));
        assert_eq!(price_for("deepseek-v4-flash-vision-exp"), mk_hit("0.22", "0.007", "0.66"));
        assert_eq!(price_for("deepseek-v4-pro"), mk_hit("0.66", "0.022", "1.98"));
    }

    #[test]
    fn deepseek_flash_prices_at_v4_flash_rates() {
        // Regression: DeepSeek's API returns the model as `deepseek-flash`,
        // which matched neither `deepseek-chat` nor `deepseek-v4-flash` and so
        // fell through to the default $3/$15 row (a ~13.7x overcharge).
        assert_eq!(price_for("deepseek-flash"), mk_hit("0.22", "0.007", "0.66"));
        assert_eq!(calculate_cost("deepseek-flash", 1_000_000, 0, 0), dec("0.22"));
        assert_eq!(calculate_cost("deepseek-flash", 0, 1_000_000, 0), dec("0.66"));
    }

    #[test]
    fn deepseek_reasoner_still_uses_the_default_row() {
        // Not listed on the pricing page yet — unchanged default behavior (AC5).
        assert_eq!(price_for("deepseek-reasoner"), mk("3.00", "15.00"));
    }

    #[test]
    fn cost_is_exact_decimal_math() {
        // gpt-4o, 100 prompt + 50 completion tokens:
        // (100/1M)*2.50 + (50/1M)*10.00 = 0.00025 + 0.00050 = 0.000750
        assert_eq!(calculate_cost("gpt-4o", 100, 50, 0), dec("0.000750"));
        // deepseek-chat at real rates: 1M prompt -> $0.22, 100k output -> $0.066
        assert_eq!(calculate_cost("deepseek-chat", 1_000_000, 0, 0), dec("0.22"));
        assert_eq!(calculate_cost("deepseek-chat", 0, 100_000, 0), dec("0.066"));
        assert_eq!(calculate_cost("deepseek-v4-pro", 1_000_000, 1_000_000, 0), dec("2.64"));
        // unknown model -> default row
        assert_eq!(calculate_cost("deepseek-reasoner", 1_000_000, 0, 0), dec("3.00"));
        // zero tokens cost nothing
        assert_eq!(calculate_cost("gpt-4o", 0, 0, 0), dec("0.000000"));
    }

    #[test]
    fn cache_split_reduces_deepseek_input_cost() {
        // 1000 prompt tokens, 800 served from cache:
        // (200*0.22 + 800*0.007)/1M = 44e-6 + 5.6e-6 = 0.0000496
        assert_eq!(calculate_cost("deepseek-chat", 1000, 0, 800), dec("0.0000496"));
        // Fully cached input bills at the hit rate only.
        assert_eq!(calculate_cost("deepseek-chat", 1_000_000, 0, 1_000_000), dec("0.007"));
        // Cached never exceeds prompt tokens (provider data trusted loosely).
        assert_eq!(calculate_cost("deepseek-chat", 1000, 0, 5000), dec("0.000007"));
    }

    #[test]
    fn models_without_hit_rate_ignore_cache_split() {
        // gpt-4o has no cache-hit row: cached tokens bill at the miss rate, so
        // the total is identical whether or not cache was reported (AC3).
        assert_eq!(calculate_cost("gpt-4o", 100, 50, 0), dec("0.000750"));
        assert_eq!(calculate_cost("gpt-4o", 100, 50, 60), dec("0.000750"));
        assert_eq!(calculate_cost("deepseek-reasoner", 1000, 0, 900), dec("0.003"));
    }

    #[test]
    fn cost_rounds_up_to_whole_cents() {
        assert_eq!(cost_to_cents(dec("0.004")), 1); // $0.004 -> 1 cent
        assert_eq!(cost_to_cents(dec("0.015")), 2); // $0.015 -> 2 cents (ceil)
        assert_eq!(cost_to_cents(dec("3.00")), 300);
        assert_eq!(cost_to_cents(dec("0")), 0);
    }

    #[test]
    fn mini_savings_delta_matches_table() {
        // 1000 prompt + 500 completion tokens on gpt-4o: cost difference vs
        // mini = 1000*2.35/1M + 500*9.40/1M = 0.00235 + 0.00470 = 0.007050
        assert_eq!(savings_if_mini(1000, 500), dec("0.007050"));
        assert_eq!(savings_if_mini(0, 0), dec("0"));
    }

    #[test]
    fn only_listed_models_have_an_explicit_row() {
        for priced in [
            "deepseek-flash",
            "deepseek-chat",
            "deepseek-v4-flash",
            "deepseek-v4-pro",
            "gpt-4o",
            "gpt-4o-mini",
        ] {
            assert!(is_priced(priced), "{priced} should be priced");
        }
        // Unlisted models fall back to the default row and must be detectable.
        for unpriced in ["deepseek-reasoner", "some-future-model"] {
            assert!(!is_priced(unpriced), "{unpriced} should be flagged as unpriced");
            assert_eq!(price_for(unpriced), mk("3.00", "15.00"));
        }
    }

    #[test]
    fn anthropic_models_are_priced() {
        // ITEM-050: Claude rows so Messages traffic isn't defaulted to $3/$15.
        assert_eq!(
            price_for("claude-3-5-sonnet-20241022"),
            mk_hit("3.00", "0.30", "15.00")
        );
        assert_eq!(
            price_for("claude-sonnet-4-5-20250929"),
            mk_hit("3.00", "0.30", "15.00")
        );
        assert_eq!(
            price_for("claude-3-5-haiku-20241022"),
            mk_hit("0.80", "0.08", "4.00")
        );
        assert_eq!(price_for("claude-3-opus-20240229"), mk_hit("15.00", "1.50", "75.00"));
        assert!(is_priced("claude-3-5-sonnet-20241022"));
        // 1M in + 1M out on sonnet = 3 + 15 = 18.
        assert_eq!(
            calculate_cost("claude-3-5-sonnet-20241022", 1_000_000, 1_000_000, 0),
            dec("18.00")
        );
    }

    #[test]
    fn savings_vs_is_the_cheaper_serve_delta_and_never_negative() {
        // gpt-4o -> gpt-4o-mini on 1M prompt: 2.50 - 0.15 = 2.35.
        assert_eq!(savings_vs("gpt-4o", "gpt-4o-mini", 1_000_000, 0, 0), dec("2.35"));
        // The reverse (a more expensive substitute) is not a saving.
        assert_eq!(savings_vs("gpt-4o-mini", "gpt-4o", 1_000_000, 0, 0), Decimal::ZERO);
        // Same model -> nothing.
        assert_eq!(savings_vs("deepseek-flash", "deepseek-flash", 1_000_000, 0, 0), Decimal::ZERO);
        // deepseek-v4-pro -> deepseek-flash, 1M in + 1M out: (0.66-0.22)+(1.98-0.66) = 1.76.
        assert_eq!(
            savings_vs("deepseek-v4-pro", "deepseek-flash", 1_000_000, 1_000_000, 0),
            dec("1.76")
        );
    }

    /// Minimal `MakeWriter` so the warning test can read what was logged.
    #[derive(Clone, Default)]
    struct CapturedLog(std::sync::Arc<std::sync::Mutex<String>>);

    impl std::io::Write for CapturedLog {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().push_str(&String::from_utf8_lossy(buf));
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLog {
        type Writer = CapturedLog;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn an_unpriced_model_warns_once_naming_the_default_row() {
        let log = CapturedLog::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(log.clone())
            .with_max_level(tracing::Level::WARN)
            .finish();
        let (priced, first, second) = tracing::subscriber::with_default(subscriber, || {
            (
                warn_if_unpriced("deepseek-flash"),
                warn_if_unpriced("future-model-2099"),
                warn_if_unpriced("future-model-2099"),
            )
        });
        assert!(!priced, "a priced model is never flagged");
        assert!(first, "an unpriced model warns on first sighting");
        assert!(!second, "and is deduped on the next");
        let output = log.0.lock().unwrap().clone();
        assert!(
            !output.contains("deepseek-flash"),
            "no warning for a priced model: {output}"
        );
        assert_eq!(
            output.matches("future-model-2099").count(),
            1,
            "exactly one line: {output}"
        );
        assert!(
            output.contains("default"),
            "names the default fallback: {output}"
        );
    }
