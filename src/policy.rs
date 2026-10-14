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

pub type StoreResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

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

/// One stored policy version for a tenant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    pub version: i64,
    /// The adaptive router threshold (None = leave the static config alone).
    pub router_threshold: Option<f64>,
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
        let row: Option<(i64, Option<f64>, serde_json::Value, bool)> = sqlx::query_as(
            "SELECT version, router_threshold, evidence, enabled
             FROM tenant_policies WHERE tenant_id = $1 AND enabled
             ORDER BY version DESC LIMIT 1",
        )
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(version, router_threshold, evidence, enabled)| Policy {
            version,
            router_threshold,
            evidence,
            enabled,
        }))
    }

    async fn publish(&self, tenant_id: Uuid, policy: Policy) -> StoreResult<Policy> {
        let row: (i64, Option<f64>, serde_json::Value, bool) = sqlx::query_as(
            "INSERT INTO tenant_policies (tenant_id, version, router_threshold, evidence, enabled)
             VALUES ($1, COALESCE((SELECT MAX(version) FROM tenant_policies WHERE tenant_id = $1), 0) + 1, $2, $3, $4)
             RETURNING version, router_threshold, evidence, enabled",
        )
        .bind(tenant_id)
        .bind(policy.router_threshold)
        .bind(sqlx::types::Json(policy.evidence))
        .bind(policy.enabled)
        .fetch_one(&self.pool)
        .await?;
        Ok(Policy {
            version: row.0,
            router_threshold: row.1,
            evidence: row.2,
            enabled: row.3,
        })
    }

    async fn rollback(&self, tenant_id: Uuid, version: i64) -> StoreResult<Option<Policy>> {
        let source: Option<(Option<f64>, serde_json::Value)> = sqlx::query_as(
            "SELECT router_threshold, evidence FROM tenant_policies
             WHERE tenant_id = $1 AND version = $2",
        )
        .bind(tenant_id)
        .bind(version)
        .fetch_optional(&self.pool)
        .await?;
        let Some((router_threshold, evidence)) = source else {
            return Ok(None);
        };
        self.publish(
            tenant_id,
            Policy {
                version: 0,
                router_threshold,
                evidence,
                enabled: true,
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
        match analyse(pool, tenant_id, current).await {
            Ok(Some((next, evidence))) => {
                let policy = Policy {
                    version: 0,
                    router_threshold: Some(next),
                    evidence,
                    enabled: true,
                };
                match store.publish(tenant_id, policy).await {
                    Ok(published) => tracing::info!(
                        "adaptive policy v{} for {tenant_id}: router threshold {current:.3} -> {next:.3}",
                        published.version
                    ),
                    Err(e) => tracing::warn!("adaptive policy publish failed for {tenant_id}: {e}"),
                }
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("adaptive policy analysis failed for {tenant_id}: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn no_move_without_enough_evidence() {
        // Too few downgraded requests, or no baseline: keep the current value.
        assert_eq!(recommend_threshold(0.35, MIN_SAMPLE - 1, 5, Some(0.9)), 0.35);
        assert_eq!(recommend_threshold(0.35, 100, 90, None), 0.35);
    }

    #[test]
    fn a_worse_cheap_tier_raises_the_threshold() {
        // 60% success on downgraded vs 90% baseline -> downgrade less.
        assert!(approx(recommend_threshold(0.35, 100, 60, Some(0.9)), 0.40));
    }

    #[test]
    fn an_every_bit_as_good_cheap_tier_lowers_the_threshold() {
        // 92% on downgraded vs 90% baseline -> downgrade more.
        assert!(approx(recommend_threshold(0.35, 100, 92, Some(0.9)), 0.30));
    }

    #[test]
    fn the_step_is_bounded_at_the_limits() {
        assert_eq!(recommend_threshold(0.0, 100, 99, Some(0.9)), 0.0);
        assert_eq!(recommend_threshold(0.9, 100, 10, Some(0.9)), 0.9);
    }

    #[tokio::test]
    async fn versions_are_append_only_and_rollback_restores() {
        let tenant = Uuid::new_v4();
        let store = InMemoryPolicyStore::default();
        assert_eq!(store.active(tenant).await.unwrap(), None);
        let v1 = store
            .publish(tenant, Policy { version: 0, router_threshold: Some(0.30), evidence: json!({"why": "v1"}), enabled: true })
            .await
            .unwrap();
        assert_eq!(v1.version, 1);
        let v2 = store
            .publish(tenant, Policy { version: 0, router_threshold: Some(0.35), evidence: json!({"why": "v2"}), enabled: true })
            .await
            .unwrap();
        assert_eq!(v2.version, 2);
        assert_eq!(store.active(tenant).await.unwrap().unwrap().version, 2);
        // Rollback restores v1's threshold as a NEW version (auditable).
        let restored = store.rollback(tenant, 1).await.unwrap().unwrap();
        assert_eq!(restored.version, 3);
        assert_eq!(restored.router_threshold, Some(0.30));
        assert_eq!(store.active(tenant).await.unwrap().unwrap().version, 3);
        // The kill switch freezes adaptation.
        store.freeze(tenant).await.unwrap();
        assert_eq!(store.active(tenant).await.unwrap(), None);
    }
}
