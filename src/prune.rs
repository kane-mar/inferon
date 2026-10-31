//! Opt-in context pruning (ITEM-091).
//!
//! The router rewrites the *model*; this rewrites the *messages*, and that
//! changes the answer — so it is off unless a tenant publishes a policy that
//! turns it on, a share of the traffic is kept unpruned as a holdout, and every
//! rewrite is attributed on the trace.
//!
//! Rules are deliberately conservative: exact duplicate turns go, stale turns go
//! only under an explicit token budget, and the last turn (the question being
//! asked) and every tool definition are never touched.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cost;

/// The per-tenant pruning policy. `enabled: false` is the default everywhere.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrunePolicy {
    pub enabled: bool,
    /// Drop turns whose (role, content) repeats an earlier turn.
    pub drop_duplicate_turns: bool,
    /// Estimated prompt-token ceiling (chars/4); the oldest stale turns go first.
    pub max_prompt_tokens: Option<usize>,
    /// Share of requests sent **unpruned**, to measure the effect (5–10%).
    pub holdout_percent: u8,
}

impl Default for PrunePolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            drop_duplicate_turns: true,
            max_prompt_tokens: None,
            holdout_percent: 10,
        }
    }
}

impl PrunePolicy {
    /// The holdout is clamped so a mistake can never hold out everything.
    fn holdout(&self) -> u8 {
        self.holdout_percent.min(50)
    }
}

/// What was removed from the payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removal {
    DuplicateTurn,
    StaleTurn,
}

impl Removal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DuplicateTurn => "duplicate_turn",
            Self::StaleTurn => "stale_turn",
        }
    }
}

/// The result of a rewrite: the payload to send, what was removed, and the
/// identity of the payload it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Rewrite {
    pub payload: Value,
    pub removals: Vec<Removal>,
    pub original_hash: String,
    pub bytes_saved: usize,
}

impl Rewrite {
    /// The trace record (`traces.prune_event`): what changed, measured against
    /// the original payload, priced with the same engine as the request.
    pub fn event(&self, model: &str, version: i64) -> Value {
        let tokens_saved = (self.bytes_saved / 4) as i64;
        serde_json::json!({
            "policy_version": version,
            "kinds": {
                Removal::DuplicateTurn.as_str(): self.removals.iter().filter(|r| **r == Removal::DuplicateTurn).count(),
                Removal::StaleTurn.as_str(): self.removals.iter().filter(|r| **r == Removal::StaleTurn).count(),
            },
            "turns_removed": self.removals.len(),
            "bytes_saved": self.bytes_saved,
            "tokens_saved_est": tokens_saved,
            "usd_saved_est": cost::calculate_cost(model, cost::tokens_as_i32(tokens_saved), 0, 0).round_dp(10).to_string(),
            "original_prompt_hash": self.original_hash,
        })
    }
}

/// Would this request be held out (sent unpruned) for measurement?
///
/// Deterministic on the request's identity, so the same prompt always lands on
/// the same side of the split and the comparison is not noise.
pub fn in_holdout(seed: &str, policy: &PrunePolicy) -> bool {
    let percent = policy.holdout() as u32;
    if percent == 0 {
        return false;
    }
    let hash = crate::sha256_hex(seed);
    let bucket = u32::from_str_radix(&hash[..8], 16).unwrap_or(0) % 100;
    bucket < percent
}

/// Rewrite the payload, or `None` when nothing would be removed.
///
/// Returns `None` (the caller forwards the original) for a disabled policy, a
/// payload without messages, or a payload that is already minimal — a rewrite
/// failure must never fail a request.
pub fn rewrite(payload: &Value, policy: &PrunePolicy) -> Option<Rewrite> {
    if !policy.enabled {
        return None;
    }
    let messages = payload["messages"].as_array()?;
    // Nothing to remove from a single turn, and the last turn is the question.
    if messages.len() < 2 {
        return None;
    }
    let last = messages.len() - 1;
    let protected = |index: usize| index == last || messages[index]["role"] == "system";

    let original_hash = crate::sha256_hex(&payload.to_string());
    let mut keep = vec![true; messages.len()];
    let mut removals = Vec::new();

    if policy.drop_duplicate_turns {
        let mut seen = std::collections::HashSet::new();
        for (index, message) in messages.iter().enumerate() {
            if protected(index) {
                continue;
            }
            let content = message["content"].as_str().unwrap_or_default();
            if content.is_empty() {
                continue;
            }
            let identity = (message["role"].as_str().unwrap_or_default(), content);
            if !seen.insert(identity) {
                keep[index] = false;
                removals.push(Removal::DuplicateTurn);
            }
        }
    }

    if let Some(budget) = policy.max_prompt_tokens {
        // Estimate the prompt as chars/4 over every kept turn, then drop the
        // oldest droppable turn until it fits.
        let mut tokens: usize = messages
            .iter()
            .enumerate()
            .filter(|(index, _)| keep[*index])
            .map(|(_, message)| message_chars(message) / 4)
            .sum();
        for index in 0..messages.len() {
            if tokens <= budget {
                break;
            }
            if !keep[index] || protected(index) {
                continue;
            }
            keep[index] = false;
            tokens -= message_chars(&messages[index]) / 4;
            removals.push(Removal::StaleTurn);
        }
    }

    if removals.is_empty() {
        return None;
    }

    let kept: Vec<Value> = messages
        .iter()
        .enumerate()
        .filter(|(index, _)| keep[*index])
        .map(|(_, message)| message.clone())
        .collect();
    let mut pruned = payload.clone();
    pruned["messages"] = Value::Array(kept);

    let bytes_saved = payload.to_string().len().saturating_sub(pruned.to_string().len());
    Some(Rewrite {
        payload: pruned,
        removals,
        original_hash,
        bytes_saved,
    })
}

/// Characters the model reads for a message (string or content parts).
fn message_chars(message: &Value) -> usize {
    match &message["content"] {
        Value::String(text) => text.len(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .map(str::len)
            .sum(),
        _ => 0,
    }
}

/// Minimum requests per arm before the holdout comparison is a verdict.
pub const MIN_SAMPLE: i64 = 20;
/// How much worse the pruned arm may be before the policy is turned off.
pub const REGRESSION_MARGIN: f64 = 0.05;

/// Should the policy be disabled? The pruned arm has to be measurably worse
/// than the held-out arm — a smaller sample, or a tie, is no verdict.
pub fn should_disable(
    pruned_ok: i64,
    pruned_total: i64,
    holdout_ok: i64,
    holdout_total: i64,
) -> bool {
    if pruned_total < MIN_SAMPLE || holdout_total < MIN_SAMPLE {
        return false;
    }
    let pruned = pruned_ok as f64 / pruned_total as f64;
    let holdout = holdout_ok as f64 / holdout_total as f64;
    pruned + REGRESSION_MARGIN < holdout
}

/// Compare the pruned and held-out arms over the last day and turn the policy
/// off when pruning measurably hurts quality. Returns the number of tenants
/// disabled.
///
/// This is the item's kill switch: savings never outrank a worse answer.
pub async fn evaluate(pool: &sqlx::PgPool) -> usize {
    let rows: Vec<(uuid::Uuid, i64, i64, i64, i64, i64)> = match sqlx::query_as(
        "SELECT tenant_id,
                COUNT(*) FILTER (WHERE NOT held_out AND outcome = 'ok'),
                COUNT(*) FILTER (WHERE NOT held_out),
                COUNT(*) FILTER (WHERE held_out AND outcome = 'ok'),
                COUNT(*) FILTER (WHERE held_out),
                MAX((prune_event->>'policy_version')::bigint)
         FROM traces,
              LATERAL (SELECT (prune_event->>'held_out')::boolean AS held_out) AS arm
         WHERE prune_event IS NOT NULL
           AND prune_event->>'held_out' IS NOT NULL
           AND created_at >= NOW() - INTERVAL '24 hours'
         GROUP BY tenant_id
         HAVING COUNT(*) FILTER (WHERE held_out) >= $1
            AND COUNT(*) FILTER (WHERE NOT held_out) >= $1",
    )
    .bind(MIN_SAMPLE)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!("prune evaluation failed: {e}");
            return 0;
        }
    };

    let store = crate::policy::PostgresPolicyStore::new(pool.clone());
    let mut disabled = 0usize;
    for (tenant_id, pruned_ok, pruned_total, holdout_ok, holdout_total, version) in rows {
        if !should_disable(pruned_ok, pruned_total, holdout_ok, holdout_total) {
            continue;
        }
        let Ok(Some(current)) = crate::policy::PolicyStore::active(&store, tenant_id).await else {
            continue;
        };
        if current.prune.as_ref().is_none_or(|prune| !prune.enabled) {
            continue;
        }
        let evidence = serde_json::json!({
            "why": "pruning_regressed",
            "pruned_ok": pruned_ok,
            "pruned_requests": pruned_total,
            "holdout_ok": holdout_ok,
            "holdout_requests": holdout_total,
            "disabled_version": version,
        });
        let next = crate::policy::Policy {
            prune: current.prune.clone().map(|prune| PrunePolicy { enabled: false, ..prune }),
            evidence,
            ..current
        };
        match crate::policy::PolicyStore::publish(&store, tenant_id, next).await {
            Ok(published) => {
                disabled += 1;
                tracing::warn!(
                    tenant_id = %tenant_id,
                    version = published.version,
                    pruned_ok, pruned_total, holdout_ok, holdout_total,
                    "pruning regressed: policy disabled automatically"
                );
            }
            Err(e) => tracing::error!("prune kill switch failed for {tenant_id}: {e}"),
        }
    }
    disabled
}

#[cfg(test)]
mod tests;
