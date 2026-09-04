//! Per-request cost accounting (ITEM-006, AC4/AC6).
//!
//! Prices are USD per 1M tokens. Math is rust_decimal — never float.
//! Source: implementation spec cost table; per the spec's open note, DeepSeek
//! (our default upstream) has no row yet and falls through to the default rate.

use rust_decimal::Decimal;
use std::str::FromStr;

/// Price per 1M tokens: (prompt, completion) USD.
pub type Price = (Decimal, Decimal);

/// Model-aware price lookup (USD per 1M tokens, prompt/completion).
/// Prefix-tolerant so dated snapshots price correctly. DeepSeek rows from the
/// DeepSeek pricing page (2026): `deepseek-chat` is an alias of v4-flash.
/// NOTE: single off-peak cache-miss pair — DeepSeek bills peak at 2x and
/// cache hits far lower, so these are labeled estimates, not invoices.
pub fn price_for(model: &str) -> Price {
    if model.starts_with("deepseek-v4-pro") {
        (dec("0.66"), dec("1.98"))
    } else if model.starts_with("deepseek-chat") || model.starts_with("deepseek-v4-flash") {
        (dec("0.22"), dec("0.66"))
    } else if model.starts_with("gpt-4o-mini") {
        (dec("0.15"), dec("0.60"))
    } else if model.starts_with("gpt-4o") {
        (dec("2.50"), dec("10.00"))
    } else {
        (dec("3.00"), dec("15.00"))
    }
}

/// Upstream cost of a request in USD, from the per-1M-token price table.
/// Formula: prompt/1M * p_rate + completion/1M * c_rate.
pub fn calculate_cost(model: &str, prompt_tokens: i32, completion_tokens: i32) -> Decimal {
    let (p_rate, c_rate) = price_for(model);
    let per_million = Decimal::from(1_000_000);
    let p_cost = (Decimal::from(prompt_tokens) / per_million) * p_rate;
    let c_cost = (Decimal::from(completion_tokens) / per_million) * c_rate;
    p_cost + c_cost
}

/// Estimated saving if a gpt-4o-tier request had run on gpt-4o-mini
/// (price delta applied to the actual token counts).
pub fn savings_if_mini(prompt_tokens: i64, completion_tokens: i64) -> Decimal {
    let per_million = Decimal::from(1_000_000);
    // 2.50 - 0.15 = 2.35 ; 10.00 - 0.60 = 9.40
    (Decimal::from(prompt_tokens) * dec("2.35") / per_million)
        + (Decimal::from(completion_tokens) * dec("9.40") / per_million)
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).expect("static price literal")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_match_the_spec_table() {
        assert_eq!(price_for("gpt-4o"), (dec("2.50"), dec("10.00")));
        assert_eq!(price_for("gpt-4o-mini"), (dec("0.15"), dec("0.60")));
        assert_eq!(price_for("gpt-4o-2024-08-06"), (dec("2.50"), dec("10.00")));
        assert_eq!(price_for("gpt-4o-mini-2024-07-18"), (dec("0.15"), dec("0.60")));
        assert_eq!(price_for("unknown-model"), (dec("3.00"), dec("15.00")));
    }

    #[test]
    fn deepseek_chat_prices_at_v4_flash_rates() {
        // Oracle: DeepSeek pricing page — v4-flash input (cache miss) $0.22 /
        // output $0.66 off-peak per 1M; deepseek-chat resolves to v4-flash.
        assert_eq!(price_for("deepseek-chat"), (dec("0.22"), dec("0.66")));
        assert_eq!(price_for("deepseek-v4-flash"), (dec("0.22"), dec("0.66")));
        assert_eq!(price_for("deepseek-v4-flash-vision-exp"), (dec("0.22"), dec("0.66")));
        assert_eq!(price_for("deepseek-v4-pro"), (dec("0.66"), dec("1.98")));
    }

    #[test]
    fn deepseek_reasoner_still_uses_the_default_row() {
        // Not listed on the pricing page yet — unchanged default behavior (AC5).
        assert_eq!(price_for("deepseek-reasoner"), (dec("3.00"), dec("15.00")));
    }

    #[test]
    fn cost_is_exact_decimal_math() {
        // gpt-4o, 100 prompt + 50 completion tokens:
        // (100/1M)*2.50 + (50/1M)*10.00 = 0.00025 + 0.00050 = 0.000750
        assert_eq!(calculate_cost("gpt-4o", 100, 50), dec("0.000750"));
        // deepseek-chat at real rates: 1M prompt -> $0.22, 100k output -> $0.066
        assert_eq!(calculate_cost("deepseek-chat", 1_000_000, 0), dec("0.22"));
        assert_eq!(calculate_cost("deepseek-chat", 0, 100_000), dec("0.066"));
        assert_eq!(calculate_cost("deepseek-v4-pro", 1_000_000, 1_000_000), dec("2.64"));
        // unknown model -> default row
        assert_eq!(calculate_cost("deepseek-reasoner", 1_000_000, 0), dec("3.00"));
        // zero tokens cost nothing
        assert_eq!(calculate_cost("gpt-4o", 0, 0), dec("0.000000"));
    }

    #[test]
    fn mini_savings_delta_matches_table() {
        // 1000 prompt + 500 completion tokens on gpt-4o: cost difference vs
        // mini = 1000*2.35/1M + 500*9.40/1M = 0.00235 + 0.00470 = 0.007050
        assert_eq!(savings_if_mini(1000, 500), dec("0.007050"));
        assert_eq!(savings_if_mini(0, 0), dec("0"));
    }
}
