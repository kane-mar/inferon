//! Per-request cost accounting (ITEM-006, AC4/AC6; cache-aware per ITEM-017).
//!
//! Prices are USD per 1M tokens. Math is rust_decimal — never float.
//! Source: implementation spec cost table + DeepSeek pricing page (2026).
//!
//! DeepSeek rows carry a separate cache-hit input rate: the provider
//! auto-caches long prefixes and bills cached input at ~3% of the miss rate,
//! so cost splits prompt tokens as `(prompt - cached) x miss + cached x hit`.
//! Models without a hit row (gpt-4o tier, default) bill every prompt token at
//! their single rate regardless of reported cache, keeping ITEM-006 numbers
//! byte-identical for them.
//!
//! NOTE: single off-peak rate pairs — DeepSeek bills peak at 2x, so these are
//! labeled estimates, not invoices.

use rust_decimal::Decimal;
use std::str::FromStr;

/// Per-1M-token price row: prompt input at cache miss, optional cache-hit
/// input rate, completion output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Price {
    pub prompt_miss: Decimal,
    pub prompt_hit: Option<Decimal>,
    pub completion: Decimal,
}

/// Model-aware price lookup (USD per 1M tokens). Prefix-tolerant so dated
/// snapshots price correctly. `deepseek-chat` is an alias of v4-flash.
pub fn price_for(model: &str) -> Price {
    if model.starts_with("deepseek-v4-pro") {
        hit("0.66", "0.022", "1.98")
    } else if model.starts_with("deepseek-chat") || model.starts_with("deepseek-v4-flash") {
        hit("0.22", "0.007", "0.66")
    } else if model.starts_with("gpt-4o-mini") {
        miss_only("0.15", "0.60")
    } else if model.starts_with("gpt-4o") {
        miss_only("2.50", "10.00")
    } else {
        miss_only("3.00", "15.00")
    }
}

/// Upstream cost of a request in USD. `cached_tokens` (provider-reported
/// `prompt_tokens_details.cached_tokens`) split off at the hit rate when the
/// model has one; otherwise cached tokens bill at the miss rate (no change vs
/// a single rate). Cached is clamped to prompt tokens.
pub fn calculate_cost(model: &str, prompt_tokens: i32, completion_tokens: i32, cached_tokens: i64) -> Decimal {
    let price = price_for(model);
    let per_million = Decimal::from(1_000_000);
    let prompt = Decimal::from(prompt_tokens);
    let cached = Decimal::from(cached_tokens.clamp(0, i64::from(prompt_tokens)));
    let miss = (prompt - cached).max(Decimal::ZERO);
    let hit_rate = price.prompt_hit.unwrap_or(price.prompt_miss);

    (miss / per_million) * price.prompt_miss
        + (cached / per_million) * hit_rate
        + (Decimal::from(completion_tokens) / per_million) * price.completion
}

/// Estimated saving if a gpt-4o-tier request had run on gpt-4o-mini
/// (price delta applied to the actual token counts).
pub fn savings_if_mini(prompt_tokens: i64, completion_tokens: i64) -> Decimal {
    let per_million = Decimal::from(1_000_000);
    // 2.50 - 0.15 = 2.35 ; 10.00 - 0.60 = 9.40
    (Decimal::from(prompt_tokens) * dec("2.35") / per_million)
        + (Decimal::from(completion_tokens) * dec("9.40") / per_million)
}

/// Round a USD cost up to whole cents (never undercount spend for budgets).
pub fn cost_to_cents(cost: Decimal) -> i64 {
    let cents = (cost * Decimal::from(100)).ceil().round_dp(0);
    i64::try_from(cents).unwrap_or(i64::MAX)
}

fn miss_only(prompt_miss: &str, completion: &str) -> Price {
    Price {
        prompt_miss: dec(prompt_miss),
        prompt_hit: None,
        completion: dec(completion),
    }
}

fn hit(prompt_miss: &str, prompt_hit: &str, completion: &str) -> Price {
    Price {
        prompt_miss: dec(prompt_miss),
        prompt_hit: Some(dec(prompt_hit)),
        completion: dec(completion),
    }
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).expect("static price literal")
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
