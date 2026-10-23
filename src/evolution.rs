//! ITEM-031 slice 1 — drift detection and the evolution job queue.
//!
//! The guard already records, per trace, whether a cheap model's answer was
//! rejected and escalated: `guard_event.outcome` is `fallback` or
//! `fallback_failed` on a rejection (and `flushed` on a pass), keyed by
//! `guarded_model`. A rising fallback rate on one model is concept drift — its
//! answers no longer satisfy the contracts it is asked to serve.
//!
//! This slice detects that drift, queues one deduped job per (tenant, model),
//! and exposes the queue to operators. The training/testing/hot-swap half (the
//! ITEM-031 ACs that need the Slice 3 infra) records its transitions in the same
//! job lifecycle.

use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::Mutex;
use uuid::Uuid;

/// A fallback rate above 5% on a model is drift (ITEM-031 AC2).
pub const DRIFT_FALLBACK_RATE: f64 = 0.05;
/// Below this many guarded responses a rate is noise, not drift.
pub const DRIFT_MIN_REQUESTS: i64 = 20;
/// The lookback window for a drift decision.
pub const WINDOW_HOURS: i64 = 24;

/// A job in one of these states is still open; a new drift signal must not
/// queue a duplicate.
const OPEN_STATES: [&str; 4] = ["queued", "training", "testing", "promoting"];

/// One model's guard evidence over the window.
#[derive(Clone, Debug, PartialEq)]
pub struct DriftSignal {
    pub adapter: String,
    pub requests: i64,
    pub fallbacks: i64,
}

impl DriftSignal {
    pub fn rate(&self) -> f64 {
        if self.requests <= 0 {
            0.0
        } else {
            self.fallbacks as f64 / self.requests as f64
        }
    }

    /// Enough evidence, and a fallback rate above the drift threshold.
    pub fn is_drifted(&self) -> bool {
        self.requests >= DRIFT_MIN_REQUESTS && self.rate() > DRIFT_FALLBACK_RATE
    }
}

/// A queued evolution job as shown to operators (no secret material).
#[derive(Clone, Debug, PartialEq)]
pub struct EvolutionJob {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub adapter: String,
    pub reason: String,
    pub state: String,
    pub evidence: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[async_trait::async_trait]
pub trait EvolutionStore: Send + Sync {
    /// Queue `adapter` unless a job is already open for it. `true` when queued.
    async fn enqueue(
        &self,
        tenant_id: Uuid,
        adapter: &str,
        evidence: &Value,
    ) -> Result<bool, String>;
    async fn list(&self, tenant_id: Uuid) -> Result<Vec<EvolutionJob>, String>;
}

/// The live store.
pub struct PostgresEvolutionStore {
    pool: PgPool,
}

impl PostgresEvolutionStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl EvolutionStore for PostgresEvolutionStore {
    async fn enqueue(
        &self,
        tenant_id: Uuid,
        adapter: &str,
        evidence: &Value,
    ) -> Result<bool, String> {
        // The partial unique index arbitrates; DO NOTHING makes a pass idempotent.
        let inserted: Option<(Uuid,)> = sqlx::query_as(
            "INSERT INTO evolution_jobs (tenant_id, adapter, reason, state, evidence)
             VALUES ($1, $2, 'drift', 'queued', $3)
             ON CONFLICT DO NOTHING
             RETURNING id",
        )
        .bind(tenant_id)
        .bind(adapter)
        .bind(evidence)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(inserted.is_some())
    }

    async fn list(&self, tenant_id: Uuid) -> Result<Vec<EvolutionJob>, String> {
        let rows: Vec<(Uuid, Uuid, String, String, String, Value, String, String)> =
            sqlx::query_as(
                "SELECT id, tenant_id, adapter, reason, state, evidence,
                        created_at::text, updated_at::text
                   FROM evolution_jobs
                  WHERE tenant_id = $1
                  ORDER BY created_at DESC",
            )
            .bind(tenant_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|(id, tenant_id, adapter, reason, state, evidence, created_at, updated_at)| {
                EvolutionJob {
                    id,
                    tenant_id,
                    adapter,
                    reason,
                    state,
                    evidence,
                    created_at,
                    updated_at,
                }
            })
            .collect())
    }
}

/// The test double (and a seam for a future in-memory edge store).
#[derive(Default)]
pub struct InMemoryEvolutionStore {
    jobs: Mutex<Vec<EvolutionJob>>,
}

#[async_trait::async_trait]
impl EvolutionStore for InMemoryEvolutionStore {
    async fn enqueue(
        &self,
        tenant_id: Uuid,
        adapter: &str,
        evidence: &Value,
    ) -> Result<bool, String> {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs.iter().any(|job| {
            job.tenant_id == tenant_id
                && job.adapter == adapter
                && OPEN_STATES.contains(&job.state.as_str())
        }) {
            return Ok(false);
        }
        jobs.push(EvolutionJob {
            id: Uuid::new_v4(),
            tenant_id,
            adapter: adapter.to_string(),
            reason: "drift".to_string(),
            state: "queued".to_string(),
            evidence: evidence.clone(),
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
        });
        Ok(true)
    }

    async fn list(&self, tenant_id: Uuid) -> Result<Vec<EvolutionJob>, String> {
        Ok(self
            .jobs
            .lock()
            .unwrap()
            .iter()
            .filter(|job| job.tenant_id == tenant_id)
            .cloned()
            .collect())
    }
}

/// Per-model guard evidence over the window (only models with enough volume).
pub async fn detect_drift(
    pool: &PgPool,
    tenant_id: Uuid,
) -> Result<Vec<DriftSignal>, String> {
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT guard_event->>'guarded_model' AS adapter,
                count(*) AS requests,
                count(*) FILTER (
                    WHERE guard_event->>'outcome' IN ('fallback', 'fallback_failed')
                ) AS fallbacks
           FROM traces
          WHERE tenant_id = $1
            AND created_at > now() - make_interval(hours => $2::int)
            AND guard_event IS NOT NULL
            AND guard_event->>'guarded_model' IS NOT NULL
          GROUP BY 1
         HAVING count(*) >= $3
          ORDER BY 1",
    )
    .bind(tenant_id)
    .bind(WINDOW_HOURS)
    .bind(DRIFT_MIN_REQUESTS)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(adapter, requests, fallbacks)| DriftSignal {
            adapter,
            requests,
            fallbacks,
        })
        .collect())
}

/// One scheduled pass: detect drift per tenant and queue a deduped job.
/// Returns the number of jobs queued.
pub async fn run_evolution(pool: &PgPool) -> usize {
    let store = PostgresEvolutionStore::new(pool.clone());
    let tenants: Vec<(Uuid,)> = match sqlx::query_as("SELECT id FROM tenants")
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!("evolution: tenant list failed: {e}");
            return 0;
        }
    };
    let mut queued = 0;
    for (tenant_id,) in tenants {
        let signals = match detect_drift(pool, tenant_id).await {
            Ok(signals) => signals,
            Err(e) => {
                tracing::warn!("evolution: drift detection failed for {tenant_id}: {e}");
                continue;
            }
        };
        for signal in signals.into_iter().filter(DriftSignal::is_drifted) {
            let evidence = json!({
                "reason": "drift",
                "requests": signal.requests,
                "fallbacks": signal.fallbacks,
                "rate": signal.rate(),
                "window_hours": WINDOW_HOURS,
            });
            match store.enqueue(tenant_id, &signal.adapter, &evidence).await {
                Ok(true) => {
                    queued += 1;
                    tracing::info!(
                        "evolution job queued for {tenant_id}/{} ({:.1}% fallback)",
                        signal.adapter,
                        signal.rate() * 100.0
                    );
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(
                        "evolution: enqueue failed for {tenant_id}/{}: {e}",
                        signal.adapter
                    );
                }
            }
        }
    }
    queued
}

#[cfg(test)]
mod tests;
