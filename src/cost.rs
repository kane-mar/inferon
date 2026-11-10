// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

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
//!
//! A model with no matching row is costed at the default $3.00/$15.00 per 1M
//! and flagged once per model via [`warn_if_unpriced`] — a silent fall-through
//! here once overcharged `deepseek-flash` ~13.7x.

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

/// Explicit price row for a model, or `None` when it is absent from the table.
fn lookup(model: &str) -> Option<Price> {
    if model.starts_with("deepseek-v4-pro") {
        Some(hit("0.66", "0.022", "1.98"))
    } else if model.starts_with("deepseek-chat")
        || model.starts_with("deepseek-flash")
        || model.starts_with("deepseek-v4-flash")
    {
        Some(hit("0.22", "0.007", "0.66"))
    } else if model.starts_with("claude-3-5-sonnet")
        || model.starts_with("claude-3-7-sonnet")
        || model.starts_with("claude-sonnet-4")
    {
        // Anthropic Messages (ITEM-050): $3 / $15, cache read $0.30.
        Some(hit("3.00", "0.30", "15.00"))
    } else if model.starts_with("claude-3-5-haiku") {
        Some(hit("0.80", "0.08", "4.00"))
    } else if model.starts_with("claude-haiku-4") {
        Some(hit("1.00", "0.10", "5.00"))
    } else if model.starts_with("claude-3-haiku") {
        Some(hit("0.25", "0.03", "1.25"))
    } else if model.starts_with("claude-opus-4") || model.starts_with("claude-3-opus") {
        Some(hit("15.00", "1.50", "75.00"))
    } else if model.starts_with("gpt-4o-mini") {
        Some(miss_only("0.15", "0.60"))
    } else if model.starts_with("gpt-4o") {
        Some(miss_only("2.50", "10.00"))
    } else {
        None
    }
}

/// Model-aware price lookup (USD per 1M tokens). Prefix-tolerant so dated
/// snapshots price correctly. `deepseek-chat` and `deepseek-flash` are aliases
/// of v4-flash. Unknown models fall back to the default row; use [`is_priced`]
/// to tell the two apart.
/// Token counts are `i32` in the pricing tables (legacy of the cost formula):
/// saturate instead of wrapping, so an absurd count can never produce a
/// negative or overflowed cost.
pub(crate) fn tokens_as_i32(tokens: i64) -> i32 {
    tokens.clamp(0, i64::from(i32::MAX)) as i32
}

pub fn price_for(model: &str) -> Price {
    lookup(model).unwrap_or_else(|| miss_only("3.00", "15.00"))
}

/// True when the model has an explicit price row (i.e. not the fallback default).
pub fn is_priced(model: &str) -> bool {
    lookup(model).is_some()
}

/// Warn, once per model, that it has no price row and is being costed at the
/// default ($3.00/M input, $15.00/M output). Returns whether this call actually
/// warned — the dedupe keeps a chatty unknown model to a single line.
pub fn warn_if_unpriced(model: &str) -> bool {
    if is_priced(model) {
        return false;
    }
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let first = SEEN
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(model.to_string());
    if first {
        crate::metrics::count_unpriced();
        tracing::warn!(
            model,
            "no price row; costed at the default $3.00/$15.00 per 1M. Add it to cost::price_for if that is wrong"
        );
    }
    first
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

/// ITEM-053: the saving a prompt cache provides — the cost with no cache minus
/// the cost with the actual cached tokens. Zero when nothing was cached (or the
/// model has no hit rate).
pub fn cache_savings(
    model: &str,
    prompt_tokens: i32,
    completion_tokens: i32,
    cached_tokens: i64,
) -> Decimal {
    let uncached = calculate_cost(model, prompt_tokens, completion_tokens, 0);
    let costed = calculate_cost(model, prompt_tokens, completion_tokens, cached_tokens);
    (uncached - costed).max(Decimal::ZERO)
}

/// Estimated saving from serving a request on `served_model` instead of the
/// `counterfactual_model` the client asked for (a router downgrade or a
/// fallback). Zero when the substitute is not cheaper (or is the same model).
pub fn savings_vs(
    counterfactual_model: &str,
    served_model: &str,
    prompt_tokens: i32,
    completion_tokens: i32,
    cached_tokens: i64,
) -> Decimal {
    let asked = calculate_cost(counterfactual_model, prompt_tokens, completion_tokens, cached_tokens);
    let served = calculate_cost(served_model, prompt_tokens, completion_tokens, cached_tokens);
    (asked - served).max(Decimal::ZERO)
}

/// The upstream cost recorded on a trace. A locally hosted model costs nothing at
/// the margin, whatever the price catalog says (ITEM-077).
/// ITEM-140: the completion a projection assumes when the caller did not say.
///
/// A refused request has no completion to price, so the projection needs a number.
/// 512 tokens is the median completion of this platform's own traffic, and it is
/// stated here — and on the page — rather than hidden, because "prevented" is an
/// estimate and an estimate that pretends to be a measurement is the one thing
/// this product must not ship.
pub const PROJECTED_COMPLETION_TOKENS: i32 = 512;

/// What a request would have cost had it run: the prompt at the model's price,
/// plus the caller's `max_tokens` (or the documented default) at the output price.
///
/// Used for the two refusals (budget, rate limit) and for the 85% downgrade's
/// delta, so every "prevented" figure on the dashboard comes from one function
/// and one assumption.
pub fn projected_cost(model: &str, prompt_tokens: i64, max_tokens: Option<i64>) -> Decimal {
    let prompt = prompt_tokens.clamp(0, i64::from(i32::MAX)) as i32;
    let completion = max_tokens
        .map(|tokens| tokens.clamp(1, i64::from(i32::MAX)) as i32)
        .unwrap_or(PROJECTED_COMPLETION_TOKENS);
    calculate_cost(model, prompt, completion, 0)
}

/// The completion a caller asked for, from the payload.
pub fn requested_max_tokens(payload: &serde_json::Value) -> Option<i64> {
    payload["max_tokens"]
        .as_i64()
        .or_else(|| payload["max_completion_tokens"].as_i64())
}

pub fn trace_cost(
    model: &str,
    prompt_tokens: i32,
    completion_tokens: i32,
    cached_tokens: i64,
    local: bool,
) -> Decimal {
    if local {
        Decimal::ZERO
    } else {
        calculate_cost(model, prompt_tokens, completion_tokens, cached_tokens)
    }
}

/// Cache savings on a trace: a local model pays no provider cache, so zero.
pub fn trace_cache_savings(
    model: &str,
    prompt_tokens: i32,
    completion_tokens: i32,
    cached_tokens: i64,
    local: bool,
) -> Decimal {
    if local {
        Decimal::ZERO
    } else {
        cache_savings(model, prompt_tokens, completion_tokens, cached_tokens)
    }
}

/// Savings versus the model the client asked for. A locally served model costs
/// nothing, so the saving is the full counterfactual (ITEM-077 AC3).
pub fn trace_savings(
    counterfactual_model: &str,
    served_model: &str,
    prompt_tokens: i32,
    completion_tokens: i32,
    cached_tokens: i64,
    local: bool,
) -> Decimal {
    let asked = calculate_cost(counterfactual_model, prompt_tokens, completion_tokens, cached_tokens);
    let served = if local {
        Decimal::ZERO
    } else {
        calculate_cost(served_model, prompt_tokens, completion_tokens, cached_tokens)
    };
    (asked - served).max(Decimal::ZERO)
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
mod tests;
