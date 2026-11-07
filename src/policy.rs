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
    /// When true (the default), the hot path uses this policy. A kill switch
    /// clears it so the static config applies again.
    #[serde(default = "default_true")]
    pub enabled: bool,
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
        enabled: true,
    }
}

/// The next router threshold, bounded. Moves **up** (downgrade less) when the
/// router's cheap tier is measurably worse than the tenant's baseline, **down**
/// (downgrade more) when it is at least as good, and not at all when the sample
/// is too small or there is no baseline.
pub fn recommend_threshold(
    current: f64,
    downgraded_requests: i64,
    downgraded_success: i64,
    baseline_success: Option<f64>,
) -> f64 {
    if downgraded_requests < MIN_SAMPLE {
        return current.clamp(MIN_THRESHOLD, MAX_THRESHOLD);
    }
    let Some(baseline) = baseline_success else {
        return current.clamp(MIN_THRESHOLD, MAX_THRESHOLD);
    };
    let downgraded = downgraded_success as f64 / downgraded_requests as f64;
    let delta = if downgraded + QUALITY_MARGIN < baseline {
        STEP
    } else if downgraded >= baseline {
        -STEP
    } else {
        0.0
    };
    (current + delta).clamp(MIN_THRESHOLD, MAX_THRESHOLD)
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
        )> = sqlx::query_as(
            "SELECT version, router_threshold, evidence, enabled, provider_weights, prune
             FROM tenant_policies WHERE tenant_id = $1 AND enabled
             ORDER BY version DESC LIMIT 1",
        )
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(version, router_threshold, evidence, enabled, weights, prune)| Policy {
            version,
            router_threshold,
            prune: prune.map(|prune| prune.0),
            evidence,
            enabled,
            provider_weights: weights.0,
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
        ) = sqlx::query_as(
            "INSERT INTO tenant_policies (tenant_id, version, router_threshold, evidence, enabled, provider_weights, prune)
             VALUES ($1, COALESCE((SELECT MAX(version) FROM tenant_policies WHERE tenant_id = $1), 0) + 1, $2, $3, $4, $5, $6)
             RETURNING version, router_threshold, evidence, enabled, provider_weights, prune",
        )
        .bind(tenant_id)
        .bind(policy.router_threshold)
        .bind(sqlx::types::Json(policy.evidence))
        .bind(policy.enabled)
        .bind(sqlx::types::Json(policy.provider_weights.clone()))
        .bind(policy.prune.clone().map(sqlx::types::Json))
        .fetch_one(&self.pool)
        .await?;
        Ok(Policy {
            version: row.0,
            router_threshold: row.1,
            prune: row.5.map(|prune| prune.0),
            evidence: row.2,
            enabled: row.3,
            provider_weights: row.4.0,
        })
    }

    async fn rollback(&self, tenant_id: Uuid, version: i64) -> StoreResult<Option<Policy>> {
        let source: Option<(
            Option<f64>,
            serde_json::Value,
            sqlx::types::Json<std::collections::BTreeMap<String, f64>>,
            Option<sqlx::types::Json<crate::prune::PrunePolicy>>,
        )> = sqlx::query_as(
            "SELECT router_threshold, evidence, provider_weights, prune FROM tenant_policies
             WHERE tenant_id = $1 AND version = $2",
        )
        .bind(tenant_id)
        .bind(version)
        .fetch_optional(&self.pool)
        .await?;
        let Some((router_threshold, evidence, weights, prune)) = source else {
            return Ok(None);
        };
        self.publish(
            tenant_id,
            Policy {
                version: 0,
                router_threshold,
                prune: prune.map(|prune| prune.0),
                evidence,
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
    let next = recommend_threshold(current, downgraded, downgraded_success, baseline);
    if (next - current).abs() < f64::EPSILON {
        return Ok(None);
    }
    Ok(Some((
        next,
        serde_json::json!({
            "reason": "router_threshold_step",
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
        let threshold = analyze_threshold(pool, tenant_id, current).await;
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
        let evidence = serde_json::json!({
            "reason": "adaptive_step",
            "from_threshold": active.as_ref().and_then(|policy| policy.router_threshold),
            "to_threshold": threshold,
            "provider_weights": provider_weights.clone(),
        });
        let policy = Policy {
            version: 0,
            prune: None,
            router_threshold: Some(threshold),
            provider_weights,
            evidence,
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
async fn analyze_threshold(pool: &sqlx::PgPool, tenant_id: Uuid, current: f64) -> f64 {
    match analyse(pool, tenant_id, current).await {
        Ok(Some((next, _))) => next,
        _ => current,
    }
}

/// Per-provider health weights from the trailing week's telemetry: quality and
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
