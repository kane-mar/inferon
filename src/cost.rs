//! Per-request cost accounting (ITEM-006, AC4/AC6).
//!
//! Prices are USD per 1M tokens. Math is rust_decimal — never float.
//! Source: implementation spec cost table; per the spec's open note, DeepSeek
//! (our default upstream) has no row yet and falls through to the default rate.

use rust_decimal::Decimal;
use std::str::FromStr;

/// Price per 1M tokens: (prompt, completion) USD.
pub type Price = (Decimal, Decimal);

/// Model-aware price lookup. Prefix-tolerant so dated snapshots like
/// `gpt-4o-2024-08-06` price like `gpt-4o`; `gpt-4o-mini*` must win over the
/// `gpt-4o` prefix.
pub fn price_for(model: &str) -> Price {
    if model.starts_with("gpt-4o-mini") {
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

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn prices_match_the_spec_table() {
        assert_eq!(price_for("gpt-4o"), (d("2.50"), d("10.00")));
        assert_eq!(price_for("gpt-4o-mini"), (d("0.15"), d("0.60")));
        assert_eq!(price_for("gpt-4o-2024-08-06"), (d("2.50"), d("10.00")));
        assert_eq!(price_for("gpt-4o-mini-2024-07-18"), (d("0.15"), d("0.60")));
        assert_eq!(price_for("deepseek-chat"), (d("3.00"), d("15.00"))); // default row
        assert_eq!(price_for("unknown-model"), (d("3.00"), d("15.00")));
    }

    #[test]
    fn cost_is_exact_decimal_math() {
        // gpt-4o, 100 prompt + 50 completion tokens:
        // (100/1M)*2.50 + (50/1M)*10.00 = 0.00025 + 0.00050 = 0.000750
        assert_eq!(calculate_cost("gpt-4o", 100, 50), d("0.000750"));
        // default row, 1,000,000 prompt tokens -> exactly $3.00
        assert_eq!(calculate_cost("deepseek-chat", 1_000_000, 0), d("3.00"));
        assert_eq!(calculate_cost("deepseek-chat", 0, 100_000), d("1.50"));
        // zero tokens cost nothing
        assert_eq!(calculate_cost("gpt-4o", 0, 0), d("0.000000"));
    }

    #[test]
    fn mini_savings_delta_matches_table() {
        // 1000 prompt + 500 completion tokens on gpt-4o: cost difference vs
        // mini = 1000*2.35/1M + 500*9.40/1M = 0.00235 + 0.00470 = 0.007050
        assert_eq!(savings_if_mini(1000, 500), d("0.007050"));
        assert_eq!(savings_if_mini(0, 0), d("0"));
    }
}
