// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! Adaptive policy loop (ITEM-064) — the loop and its governance.
//!
//! The heuristics are otherwise static and open-loop: the router threshold is
//! set by hand and never learns. This module derives a **bounded** adjustment
//! from telemetry (the quality of what the router already downshifted versus the
//! tenant's baseline), stores it as a **versioned, auditable, reversible**
//! policy, and lets the hot path read it like any other config — no redeploy.
//!
//! Guardrails: a step is bounded, a minimum sample is required before moving,
//! the frontier/fallback is never touched, and a kill switch / rollback returns
//! to a known-good version.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use crate::StoreResult;

/// Largest single-step threshold change (guardrail: no wild swings).
pub const STEP: f64 = 0.05;
/// Minimum downgraded requests before the loop will move (statistical floor).
pub const MIN_SAMPLE: i64 = 20;
/// Threshold bounds (0 = always downgrade; 0.9 = almost never).
pub const MIN_THRESHOLD: f64 = 0.0;
pub const MAX_THRESHOLD: f64 = 0.9;
/// A downgraded success rate this far below baseline means the cheap tier is
/// hurting quality.
const QUALITY_MARGIN: f64 = 0.05;

/// What the loop is optimising for (ITEM-141).
///
/// Before this, the loop had one direction: cheaper whenever quality held. That
/// is the right default for a cost product and the wrong answer for a workspace
/// whose answers matter more than their price. The objective is the workspace's
/// statement of intent, and every step the loop takes answers to it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Objective {
    /// Spend first: downgrade readily, tolerate a small quality cost.
    Cost,
    /// The default: downgrade while quality holds, stop when it does not.
    #[default]
    Balanced,
    /// Quality first: downgrade only when the cheap tier is *measurably better*,
    /// and come back up at the first sign of a shortfall.
    Quality,
}

impl Objective {
    pub fn as_str(self) -> &'static str {
        match self {
            Objective::Cost => "cost",
            Objective::Balanced => "balanced",
            Objective::Quality => "quality",
        }
    }

    /// The lowest threshold this objective will let the loop reach.
    ///
    /// A floor exists because of a live finding (2026-11-09): repeated `-STEP`
    /// steps took a workspace's threshold to 1.4e-17 — effectively "always
    /// downgrade" — with nobody able to see it. Costs-first may go low; quality-
    /// first may not go far at all.
    pub fn floor(self) -> f64 {
        match self {
            Objective::Cost => 0.05,
            Objective::Balanced => 0.15,
            Objective::Quality => 0.50,
        }
    }

    /// The downgraded success rate a step-down has to reach, given the baseline.
    ///
    /// This is the whole difference between the objectives: cost accepts a small
    /// quality cost, balanced accepts parity, quality demands a measured
    /// improvement before it will route anything to the cheap tier.
    fn down_when(self, baseline: f64) -> f64 {
        match self {
            Objective::Cost => baseline - (QUALITY_MARGIN / 2.0),
            Objective::Balanced => baseline,
            Objective::Quality => baseline + QUALITY_MARGIN,
        }
    }
}

/// A provider's health weight in `[0.1, 1.0]`: better quality and lower latency
/// (relative to the fleet median) weigh more, so a degrading provider is
/// demoted. Neutral (1.0) below the sample floor — too little data is no
/// opinion, never a silent penalty.
pub fn provider_weight(
    requests: i64,
    errors: i64,
    p95_ms: f64,
    median_p95_ms: f64,
    min_sample: i64,
) -> f64 {
    if requests < min_sample {
        return 1.0;
    }
    let quality = 1.0 - (errors as f64 / requests as f64);
    let latency = if p95_ms > 0.0 && median_p95_ms > 0.0 {
        (median_p95_ms / p95_ms).clamp(0.1, 1.0)
    } else {
        1.0
    };
    (quality * latency).clamp(0.1, 1.0)
}

/// One stored policy version for a tenant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    pub version: i64,
    /// The adaptive router threshold (None = leave the static config alone).
    pub router_threshold: Option<f64>,
    /// ITEM-069: per-provider health weights, keyed by provider id. Absent
    /// (the default) means "no opinion" — the static ordering applies.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub provider_weights: std::collections::BTreeMap<String, f64>,
    /// ITEM-091: the opt-in payload-pruning policy. Absent = pruning off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prune: Option<crate::prune::PrunePolicy>,
    /// Human-readable evidence for this version (why it changed).
    pub evidence: serde_json::Value,
    /// ITEM-141: what every step answers to. Absent (a policy written before this
    /// field existed) reads as `Balanced`, which is the behaviour it had.
    #[serde(default)]
    pub objective: Objective,
    /// When true (the default), the hot path uses this policy. A kill switch
    /// clears it so the static config applies again.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// A stored objective; an unknown string reads as the default, because a policy
/// written by a newer build must not make an older one fail to load.
fn objective_from(value: &str) -> Objective {
    match value {
        "cost" => Objective::Cost,
        "quality" => Objective::Quality,
        _ => Objective::Balanced,
    }
}

fn default_true() -> bool {
    true
}

/// ITEM-091: the policy a tenant's pruning opt-in publishes.
///
/// An existing policy keeps its router threshold and provider weights — opting
/// into pruning must not silently reset routing — and the previous version is
/// recorded in the evidence for the audit trail.
pub fn prune_policy_from(
    current: Option<&Policy>,
    body: crate::prune::PrunePolicy,
) -> Policy {
    Policy {
        version: current.map_or(0, |policy| policy.version),
        router_threshold: current.and_then(|policy| policy.router_threshold),
        provider_weights: current.map(|policy| policy.provider_weights.clone()).unwrap_or_default(),
        prune: Some(crate::prune::PrunePolicy {
            holdout_percent: body.holdout_percent.min(50),
            ..body
        }),
        evidence: serde_json::json!({
            "why": "tenant_opt_in",
            "enabled": body.enabled,
            "previous_version": current.map(|policy| policy.version),
        }),
        objective: current.map_or(Objective::Balanced, |policy| policy.objective),
        enabled: true,
    }
}

/// The next router threshold, bounded. Moves **up** (downgrade less) when the
/// router's cheap tier is measurably worse than the tenant's baseline, **down**
/// (downgrade more) when it is at least as good, and not at all when the sample
/// is too small or there is no baseline.
pub fn recommend_threshold(
    current: f64,
    objective: Objective,
    downgraded_requests: i64,
    downgraded_success: i64,
    baseline_success: Option<f64>,
) -> f64 {
    let floor = objective.floor();
    if downgraded_requests < MIN_SAMPLE {
        return current.clamp(floor, MAX_THRESHOLD);
    }
    let Some(baseline) = baseline_success else {
        return current.clamp(floor, MAX_THRESHOLD);
    };
    let downgraded = downgraded_success as f64 / downgraded_requests as f64;
    // Up (downgrade less) as soon as the cheap tier is worse than the baseline.
    // Quality mode moves twice as decisively, because a shortfall there is the
    // thing it exists to prevent.
    let up = if objective == Objective::Quality {
        2.0 * STEP
    } else {
        STEP
    };
    let delta = if downgraded + QUALITY_MARGIN < baseline {
        up
    } else if downgraded >= objective.down_when(baseline) {
        -STEP
    } else {
        0.0
    };
    (current + delta).clamp(floor, MAX_THRESHOLD)
}

/// ITEM-147: what an operator asked to change, and the rules it has to meet.
///
/// These rules lived inside the HTTP handler, so the boundary cases could only be
/// exercised as side effects of a request. They are pure here: one place to read,
/// one place to test, and the handler keeps its single job (gate, load, publish).
#[derive(Clone, Debug, Default)]
pub struct OperatorChange {
    /// `None` leaves the current value alone.
    pub enabled: Option<bool>,
    pub router_threshold: Option<f64>,
    /// The objective by name, as an operator typed it.
    pub objective: Option<String>,
    pub provider_weights: Option<std::collections::BTreeMap<String, f64>>,
}

/// Why an operator change was refused. The handler turns each into a typed 400.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyError {
    /// Outside `MIN_THRESHOLD..=MAX_THRESHOLD`. Refused, never clamped.
    InvalidThreshold,
    /// Outside `0.0..=1.0`, or not a number.
    InvalidWeight,
    /// Not one of `cost`, `balanced`, `quality`.
    UnknownObjective(String),
}

impl PolicyError {
    /// The stable code a client sees, and the sentence beside it.
    pub fn as_response(&self) -> (&'static str, String) {
        match self {
            PolicyError::InvalidThreshold => (
                "invalid_threshold",
                format!("router threshold must be between {MIN_THRESHOLD} and {MAX_THRESHOLD}"),
            ),
            PolicyError::InvalidWeight => (
                "invalid_weight",
                "provider weights must be between 0 and 1".to_string(),
            ),
            PolicyError::UnknownObjective(name) => (
                "invalid_objective",
                format!("unknown objective: {name}"),
            ),
        }
    }
}

impl OperatorChange {
    /// The policy this change produces, given the one in force.
    ///
    /// Two behaviours are deliberate: a field the operator did not name keeps its
    /// current value (the loop publishes versions too, so a threshold edit must not
    /// wipe learned weights), and the loop's own observations survive in `evidence`.
    pub fn apply(&self, current: Option<&Policy>) -> Result<Policy, PolicyError> {
        if let Some(threshold) = self.router_threshold {
            if !(MIN_THRESHOLD..=MAX_THRESHOLD).contains(&threshold) {
                return Err(PolicyError::InvalidThreshold);
            }
        }
        if let Some(weights) = &self.provider_weights {
            if weights
                .values()
                .any(|weight| !weight.is_finite() || !(0.0..=1.0).contains(weight))
            {
                return Err(PolicyError::InvalidWeight);
            }
        }
        let objective = match self.objective.as_deref() {
            Some("cost") => Objective::Cost,
            Some("quality") => Objective::Quality,
            Some("balanced") => Objective::Balanced,
            Some(other) => return Err(PolicyError::UnknownObjective(other.to_string())),
            None => current.map_or(Objective::Balanced, |policy| policy.objective),
        };

        Ok(Policy {
            version: 0,
            router_threshold: self
                .router_threshold
                .or_else(|| current.and_then(|policy| policy.router_threshold)),
            provider_weights: self
                .provider_weights
                .clone()
                .unwrap_or_else(|| current.map(|policy| policy.provider_weights.clone()).unwrap_or_default()),
            prune: current.and_then(|policy| policy.prune.clone()),
            evidence: operator_evidence(current),
            objective,
            enabled: self.enabled.unwrap_or(true),
        })
    }
}

/// The audit trail for a manual steer: who, when, and whatever the loop observed.
fn operator_evidence(current: Option<&Policy>) -> serde_json::Value {
    let mut evidence = serde_json::json!({
        "source": "operator",
        "by": "operator",
        "at": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0),
    });
    if let Some(prompt) = current.and_then(|policy| policy.evidence.get("prompt")) {
        evidence["prompt"] = prompt.clone();
    }
    evidence
}

#[async_trait::async_trait]
pub trait PolicyStore: Send + Sync {
    /// The tenant's active policy (`None` = static config applies).
    async fn active(&self, tenant_id: Uuid) -> StoreResult<Option<Policy>>;
    /// Publish a new version (version = previous + 1) and return it.
    async fn publish(&self, tenant_id: Uuid, policy: Policy) -> StoreResult<Policy>;
    /// Re-activate a previous version (rollback); `None` if unknown.
    async fn rollback(&self, tenant_id: Uuid, version: i64) -> StoreResult<Option<Policy>>;
    /// Freeze to the last-known-good policy (kill switch): disable adaptation.
    async fn freeze(&self, tenant_id: Uuid) -> StoreResult<()>;
}

/// In-process double: an append-only version list per tenant.
#[derive(Default)]
pub struct InMemoryPolicyStore {
    rows: std::sync::Mutex<Vec<(Uuid, Policy)>>,
}

#[async_trait::async_trait]
impl PolicyStore for InMemoryPolicyStore {
    async fn active(&self, tenant_id: Uuid) -> StoreResult<Option<Policy>> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, p)| *t == tenant_id && p.enabled)
            .max_by_key(|(_, p)| p.version)
            .map(|(_, p)| p.clone()))
    }

    async fn publish(&self, tenant_id: Uuid, mut policy: Policy) -> StoreResult<Policy> {
        let mut rows = self.rows.lock().unwrap();
        policy.version = rows
            .iter()
            .filter(|(t, _)| *t == tenant_id)
            .map(|(_, p)| p.version)
            .max()
            .unwrap_or(0)
            + 1;
        rows.push((tenant_id, policy.clone()));
        Ok(policy)
    }

    async fn rollback(&self, tenant_id: Uuid, version: i64) -> StoreResult<Option<Policy>> {
        let mut rows = self.rows.lock().unwrap();
        let Some(old) = rows
            .iter()
            .find(|(t, p)| *t == tenant_id && p.version == version)
            .map(|(_, p)| p.clone())
        else {
            return Ok(None);
        };
        let mut restored = old;
        restored.version = rows
            .iter()
            .filter(|(t, _)| *t == tenant_id)
            .map(|(_, p)| p.version)
            .max()
            .unwrap_or(0)
            + 1;
        rows.push((tenant_id, restored.clone()));
        Ok(Some(restored))
    }

    async fn freeze(&self, tenant_id: Uuid) -> StoreResult<()> {
        for (_, policy) in self.rows.lock().unwrap().iter_mut().filter(|(t, _)| *t == tenant_id) {
            policy.enabled = false;
        }
        Ok(())
    }
}

/// Postgres-backed policy store (versioned, append-only).
pub struct PostgresPolicyStore {
    pool: sqlx::PgPool,
}

impl PostgresPolicyStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl PolicyStore for PostgresPolicyStore {
    async fn active(&self, tenant_id: Uuid) -> StoreResult<Option<Policy>> {
        let row: Option<(
            i64,
            Option<f64>,
            serde_json::Value,
            bool,
            sqlx::types::Json<std::collections::BTreeMap<String, f64>>,
            Option<sqlx::types::Json<crate::prune::PrunePolicy>>,
            String,
        )> = sqlx::query_as(
            "SELECT version, router_threshold, evidence, enabled, provider_weights, prune, objective
             FROM tenant_policies WHERE tenant_id = $1 AND enabled
             ORDER BY version DESC LIMIT 1",
        )
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(version, router_threshold, evidence, enabled, weights, prune, objective)| Policy {
            version,
            router_threshold,
            prune: prune.map(|prune| prune.0),
            evidence,
            enabled,
            provider_weights: weights.0,
            objective: objective_from(&objective),
        }))
    }

    async fn publish(&self, tenant_id: Uuid, policy: Policy) -> StoreResult<Policy> {
        let row: (
            i64,
            Option<f64>,
            serde_json::Value,
            bool,
            sqlx::types::Json<std::collections::BTreeMap<String, f64>>,
            Option<sqlx::types::Json<crate::prune::PrunePolicy>>,
            String,
        ) = sqlx::query_as(
            "INSERT INTO tenant_policies (tenant_id, version, router_threshold, evidence, enabled, provider_weights, prune, objective)
             VALUES ($1, COALESCE((SELECT MAX(version) FROM tenant_policies WHERE tenant_id = $1), 0) + 1, $2, $3, $4, $5, $6, $7)
             RETURNING version, router_threshold, evidence, enabled, provider_weights, prune, objective",
        )
        .bind(tenant_id)
        .bind(policy.router_threshold)
        .bind(sqlx::types::Json(policy.evidence))
        .bind(policy.enabled)
        .bind(sqlx::types::Json(policy.provider_weights.clone()))
        .bind(policy.prune.clone().map(sqlx::types::Json))
        .bind(policy.objective.as_str())
        .fetch_one(&self.pool)
        .await?;
        Ok(Policy {
            version: row.0,
            router_threshold: row.1,
            prune: row.5.map(|prune| prune.0),
            evidence: row.2,
            enabled: row.3,
            provider_weights: row.4.0,
            objective: objective_from(&row.6),
        })
    }

    async fn rollback(&self, tenant_id: Uuid, version: i64) -> StoreResult<Option<Policy>> {
        let source: Option<(
            Option<f64>,
            serde_json::Value,
            sqlx::types::Json<std::collections::BTreeMap<String, f64>>,
            Option<sqlx::types::Json<crate::prune::PrunePolicy>>,
            String,
        )> = sqlx::query_as(
            "SELECT router_threshold, evidence, provider_weights, prune, objective FROM tenant_policies
             WHERE tenant_id = $1 AND version = $2",
        )
        .bind(tenant_id)
        .bind(version)
        .fetch_optional(&self.pool)
        .await?;
        let Some((router_threshold, evidence, weights, prune, objective)) = source else {
            return Ok(None);
        };
        self.publish(
            tenant_id,
            Policy {
                version: 0,
                router_threshold,
                prune: prune.map(|prune| prune.0),
                evidence,
                // A rollback restores the settings of that version, including the
                // objective it was taken under.
                objective: objective_from(&objective),
                enabled: true,
                provider_weights: weights.0,
            },
        )
        .await
        .map(Some)
    }

    async fn freeze(&self, tenant_id: Uuid) -> StoreResult<()> {
        sqlx::query("UPDATE tenant_policies SET enabled = FALSE WHERE tenant_id = $1")
            .bind(tenant_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

/// One analysis pass for a tenant: derive the bounded next threshold from the
/// quality of what the router already downshifted versus the tenant's baseline.
/// Returns `None` when there is not enough evidence to move.
pub async fn analyse(
    pool: &sqlx::PgPool,
    tenant_id: Uuid,
    current: f64,
    objective: Objective,
) -> StoreResult<Option<(f64, serde_json::Value)>> {
    let row: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*),
                COUNT(*) FILTER (WHERE outcome = 'ok'),
                COUNT(*) FILTER (WHERE router_event IS NOT NULL),
                COUNT(*) FILTER (WHERE router_event IS NOT NULL AND outcome = 'ok')
         FROM traces
         WHERE tenant_id = $1 AND created_at >= NOW() - INTERVAL '7 days'",
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?;
    let (requests, success, downgraded, downgraded_success) = row;
    let baseline = (requests > 0).then(|| success as f64 / requests as f64);
    let next = recommend_threshold(current, objective, downgraded, downgraded_success, baseline);
    if (next - current).abs() < f64::EPSILON {
        return Ok(None);
    }
    Ok(Some((
        next,
        serde_json::json!({
            "reason": "router_threshold_step",
            "objective": objective.as_str(),
            "from": current,
            "to": next,
            "requests": requests,
            "baseline_success": baseline,
            "downgraded_requests": downgraded,
            "downgraded_success": downgraded_success,
        }),
    )))
}

/// One scheduled pass of the adaptive loop: for each tenant, derive a bounded
/// next threshold from telemetry and publish a new version when it moves.
pub async fn run_adaptation(pool: &sqlx::PgPool) {
    let store = PostgresPolicyStore::new(pool.clone());
    let tenants: Vec<(Uuid,)> = match sqlx::query_as("SELECT id FROM tenants")
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!("adaptive policy: tenant list failed: {e}");
            return;
        }
    };
    for (tenant_id,) in tenants {
        let configured: Option<f64> = sqlx::query_scalar(
            "SELECT router_threshold FROM tenant_configurations WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
        let active = store.active(tenant_id).await.ok().flatten();
        let current = active
            .as_ref()
            .and_then(|policy| policy.router_threshold)
            .or(configured)
            .unwrap_or(crate::router::DOWNGRADE_THRESHOLD);
        let objective = active.as_ref().map_or(Objective::Balanced, |p| p.objective);
        let threshold = analyze_threshold(pool, tenant_id, current, objective).await;
        let provider_weights = match analyse_providers(pool, tenant_id).await {
            Ok(weights) => weights,
            Err(e) => {
                tracing::warn!("adaptive provider analysis failed for {tenant_id}: {e}");
                continue;
            }
        };
        let unchanged = active.as_ref().is_some_and(|policy| {
            policy.router_threshold == Some(threshold) && policy.provider_weights == provider_weights
        });
        if unchanged {
            continue;
        }
        // ITEM-141: the prompt is the other half of the bill. This is an
        // observation plus a proposal, never a rewrite — pruning changes answers
        // and needs the workspace's consent and a holdout to prove it.
        let prompt = analyse_prompt(pool, tenant_id).await.ok();
        let spend: rust_decimal::Decimal =
            sqlx::query_scalar("SELECT COALESCE(SUM(upstream_cost_usd), 0) FROM traces
                                WHERE tenant_id = $1 AND created_at >= NOW() - INTERVAL '7 days'")
                .bind(tenant_id)
                .fetch_one(pool)
                .await
                .unwrap_or_default();
        let pruning = prompt.as_ref().is_some_and(|plan| plan.worth_pruning(spend));
        let evidence = serde_json::json!({
            "reason": "adaptive_step",
            "objective": objective.as_str(),
            "from_threshold": active.as_ref().and_then(|policy| policy.router_threshold),
            "to_threshold": threshold,
            "provider_weights": provider_weights.clone(),
            "prompt": prompt.as_ref().map(|plan| serde_json::json!({
                "waste_usd": plan.waste_usd.round_dp(6).to_string(),
                "requests": plan.requests,
                "top_kind": plan.top_kind,
                "propose_pruning": pruning,
                "pruning_enabled": active
                    .as_ref()
                    .and_then(|policy| policy.prune.as_ref())
                    .is_some_and(|prune| prune.enabled),
            })),
        });
        let policy = Policy {
            version: 0,
            prune: active.as_ref().and_then(|policy| policy.prune.clone()),
            router_threshold: Some(threshold),
            provider_weights,
            evidence,
            objective,
            enabled: true,
        };
        match store.publish(tenant_id, policy).await {
            Ok(published) => tracing::info!(
                "adaptive policy v{} for {tenant_id}: threshold {current:.3} -> {threshold:.3}",
                published.version
            ),
            Err(e) => tracing::warn!("adaptive policy publish failed for {tenant_id}: {e}"),
        }
    }
}

/// The threshold recommendation: the moved value, or the current one when there
/// is not enough evidence to move.
async fn analyze_threshold(
    pool: &sqlx::PgPool,
    tenant_id: Uuid,
    current: f64,
    objective: Objective,
) -> f64 {
    match analyse(pool, tenant_id, current, objective).await {
        Ok(Some((next, _))) => next,
        _ => current,
    }
}

/// The prompt waste the loop can see, and what it would do about it (ITEM-141).
///
/// The loop optimised *price* only: given the same prompt it chose a cheaper
/// model. But most of the money in a runaway bill is in the prompt itself — a
/// repeated system prompt the provider cache never hits, a tool schema nobody
/// uses, boilerplate, duplicate turns. The pre-flight linter already prices all
/// four (`traces.request_lint`); this turns that into a decision.
///
/// It **proposes**, it does not rewrite: pruning changes the answer, so enabling
/// it is the workspace's call and it is measured against a holdout. The proposal
/// is published as policy evidence, which is what the console shows and what the
/// operator applies with one click.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PromptPlan {
    /// Priced waste in the window, in USD.
    pub waste_usd: rust_decimal::Decimal,
    /// Requests carrying at least one finding.
    pub requests: i64,
    /// The costliest kind, e.g. `repeated_system_prompt`.
    pub top_kind: Option<String>,
}

/// Waste worth acting on: an absolute floor, so a small workspace is not nagged
/// about cents, and it has to be a real share of what was spent.
pub const PROMPT_WASTE_MIN_USD: f64 = 1.0;
pub const PROMPT_WASTE_SHARE: f64 = 0.10;

impl PromptPlan {
    /// Whether the evidence is strong enough to propose enabling pruning.
    pub fn worth_pruning(&self, spend_usd: rust_decimal::Decimal) -> bool {
        if self.waste_usd < rust_decimal::Decimal::try_from(PROMPT_WASTE_MIN_USD).unwrap_or_default()
        {
            return false;
        }
        // A share of spend, only when spend is large enough to divide by.
        if spend_usd <= rust_decimal::Decimal::ZERO {
            return true;
        }
        let share = self.waste_usd / spend_usd;
        share >= rust_decimal::Decimal::try_from(PROMPT_WASTE_SHARE).unwrap_or_default()
    }
}

/// Aggregate the pre-flight findings for a tenant over the trailing week.
pub async fn analyse_prompt(pool: &sqlx::PgPool, tenant_id: Uuid) -> StoreResult<PromptPlan> {
    let row: (Option<rust_decimal::Decimal>, i64, Option<String>) = sqlx::query_as(
        "SELECT COALESCE(SUM((request_lint->>'wasted_usd_est')::numeric), 0),
                COUNT(*) FILTER (WHERE request_lint IS NOT NULL),
                (SELECT k.kind
                   FROM traces t2, jsonb_array_elements_text(t2.request_lint->'kinds') AS k(kind)
                  WHERE t2.tenant_id = $1
                    AND t2.created_at >= NOW() - INTERVAL '7 days'
                    AND t2.request_lint IS NOT NULL
                  GROUP BY k.kind
                  ORDER BY COUNT(*) DESC
                  LIMIT 1)
         FROM traces
         WHERE tenant_id = $1 AND created_at >= NOW() - INTERVAL '7 days'",
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?;
    Ok(PromptPlan {
        waste_usd: row.0.unwrap_or_default(),
        requests: row.1,
        top_kind: row.2,
    })
}

/// Per-provider health weights from the trailing week's telemetry/// Per-provider health weights from the trailing week's telemetry: quality and
/// latency (relative to the fleet median) decide the weight (ITEM-069).
pub async fn analyse_providers(
    pool: &sqlx::PgPool,
    tenant_id: Uuid,
) -> StoreResult<std::collections::BTreeMap<String, f64>> {
    let rows: Vec<(Uuid, i64, i64, f64)> = sqlx::query_as(
        "SELECT t.provider_id,
                COUNT(*),
                COUNT(*) FILTER (WHERE t.status_code >= 400),
                COALESCE(percentile_cont(0.95) WITHIN GROUP (ORDER BY t.total_duration_ms)::float8, 0)
         FROM traces t
         WHERE t.tenant_id = $1 AND t.provider_id IS NOT NULL
           AND t.created_at >= NOW() - INTERVAL '7 days'
         GROUP BY t.provider_id",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?;
    let mut p95s: Vec<f64> = rows.iter().map(|row| row.3).filter(|p95| *p95 > 0.0).collect();
    p95s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = p95s.get(p95s.len() / 2).copied().unwrap_or(0.0);
    Ok(rows
        .into_iter()
        .map(|(provider_id, requests, errors, p95)| {
            (
                provider_id.to_string(),
                provider_weight(requests, errors, p95, median, MIN_SAMPLE),
            )
        })
        .collect())
}

#[cfg(test)]
mod tests;
