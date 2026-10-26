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
    /// Set when the job reached a terminal state (promoted / failed).
    pub finished_at: Option<String>,
}

/// The job lifecycle (ITEM-076).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Training,
    Testing,
    Promoting,
    Promoted,
    Failed,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Training => "training",
            JobState::Testing => "testing",
            JobState::Promoting => "promoting",
            JobState::Promoted => "promoted",
            JobState::Failed => "failed",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, JobState::Promoted | JobState::Failed)
    }
}

/// A trained adapter candidate, produced by a [`Trainer`] and vetted by a [`Gate`]
/// before a [`Promoter`] makes it live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub adapter: String,
}

/// Produces a candidate adapter for a job. The real implementation (S3 + vLLM +
/// the ITEM-026 DPO dataset) is gated on the Slice 3 infra; this seam keeps the
/// loop testable without it.
#[async_trait::async_trait]
pub trait Trainer: Send + Sync {
    async fn train(&self, job: &EvolutionJob) -> Result<Candidate, String>;
}

/// The held-out gold-set gate: a candidate may only be promoted when this passes.
#[async_trait::async_trait]
pub trait Gate: Send + Sync {
    async fn passes(&self, job: &EvolutionJob, candidate: &Candidate) -> Result<bool, String>;
}

/// Issues the candidate (`load_lora_adapter`) without dropping in-flight requests.
#[async_trait::async_trait]
pub trait Promoter: Send + Sync {
    async fn promote(&self, job: &EvolutionJob, candidate: &Candidate) -> Result<(), String>;
}

/// Advance **one** queued job through train → gate → promote, recording every
/// transition so the operator view shows the whole path. Returns the terminal
/// state, or `None` when the queue is idle.
///
/// Every failure is recorded on the job with the stage that produced it; a
/// rejected candidate is never promoted.
pub async fn process_one(
    store: &dyn EvolutionStore,
    trainer: &dyn Trainer,
    gate: &dyn Gate,
    promoter: &dyn Promoter,
) -> Result<Option<JobState>, String> {
    let Some(job) = store.claim().await? else {
        return Ok(None);
    };
    // `claim` already moved the job to `training`.
    let candidate = match trainer.train(&job).await {
        Ok(candidate) => candidate,
        Err(error) => return fail(store, &job, "training", &error).await,
    };
    store
        .set_state(job.id, JobState::Testing, &json!({ "stage": "testing" }))
        .await?;
    match gate.passes(&job, &candidate).await {
        Ok(true) => {}
        Ok(false) => {
            return fail(
                store,
                &job,
                "gate",
                "the held-out gold set rejected the candidate",
            )
            .await
        }
        Err(error) => return fail(store, &job, "gate", &error).await,
    }
    store
        .set_state(job.id, JobState::Promoting, &json!({ "stage": "promoting" }))
        .await?;
    if let Err(error) = promoter.promote(&job, &candidate).await {
        return fail(store, &job, "promoting", &error).await;
    }
    store
        .set_state(
            job.id,
            JobState::Promoted,
            &json!({ "stage": "promoted", "adapter": candidate.adapter }),
        )
        .await?;
    Ok(Some(JobState::Promoted))
}

async fn fail(
    store: &dyn EvolutionStore,
    job: &EvolutionJob,
    stage: &str,
    error: &str,
) -> Result<Option<JobState>, String> {
    store
        .set_state(
            job.id,
            JobState::Failed,
            &json!({ "stage": stage, "error": error }),
        )
        .await?;
    Ok(Some(JobState::Failed))
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
    /// Take the oldest `queued` job and move it to `training` (exclusive, so two
    /// workers never claim the same job). `None` when the queue is idle.
    async fn claim(&self) -> Result<Option<EvolutionJob>, String>;
    /// Record a transition, stamping `finished_at` for terminal states. False
    /// when the id is unknown.
    async fn set_state(
        &self,
        id: Uuid,
        state: JobState,
        evidence: &Value,
    ) -> Result<bool, String>;
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
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT id, tenant_id, adapter, reason, state, evidence,
                    created_at::text, updated_at::text, finished_at::text
               FROM evolution_jobs
              WHERE tenant_id = $1
              ORDER BY created_at DESC",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(job_from_row).collect())
    }

    async fn claim(&self) -> Result<Option<EvolutionJob>, String> {
        // `FOR UPDATE SKIP LOCKED` + the state guard makes the claim exclusive:
        // two workers ticking together take different jobs.
        let row: Option<Row> = sqlx::query_as(
            "UPDATE evolution_jobs
                SET state = 'training', updated_at = now()
              WHERE id = (
                    SELECT id FROM evolution_jobs
                     WHERE state = 'queued'
                     ORDER BY created_at
                     LIMIT 1
                     FOR UPDATE SKIP LOCKED
              )
              RETURNING id, tenant_id, adapter, reason, state, evidence,
                        created_at::text, updated_at::text, finished_at::text",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(row.map(job_from_row))
    }

    async fn set_state(
        &self,
        id: Uuid,
        state: JobState,
        evidence: &Value,
    ) -> Result<bool, String> {
        let result = sqlx::query(
            "UPDATE evolution_jobs
                SET state = $2,
                    evidence = $3,
                    updated_at = now(),
                    finished_at = CASE WHEN $2 IN ('promoted', 'failed')
                                       THEN now() ELSE finished_at END
              WHERE id = $1",
        )
        .bind(id)
        .bind(state.as_str())
        .bind(evidence)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(result.rows_affected() > 0)
    }
}

/// `evolution_jobs` columns as selected above: id, tenant, adapter, reason, state,
/// evidence, created, updated, finished.
type Row = (Uuid, Uuid, String, String, String, Value, String, String, Option<String>);

fn job_from_row(row: Row) -> EvolutionJob {
    let (id, tenant_id, adapter, reason, state, evidence, created_at, updated_at, finished_at) = row;
    EvolutionJob {
        id,
        tenant_id,
        adapter,
        reason,
        state,
        evidence,
        created_at,
        updated_at,
        finished_at,
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
            finished_at: None,
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

    async fn claim(&self) -> Result<Option<EvolutionJob>, String> {
        let mut jobs = self.jobs.lock().unwrap();
        let Some(job) = jobs
            .iter_mut()
            .find(|job| job.state == JobState::Queued.as_str())
        else {
            return Ok(None);
        };
        job.state = JobState::Training.as_str().to_string();
        job.updated_at = "now".to_string();
        Ok(Some(job.clone()))
    }

    async fn set_state(&self, id: Uuid, state: JobState, evidence: &Value) -> Result<bool, String> {
        let mut jobs = self.jobs.lock().unwrap();
        let Some(job) = jobs.iter_mut().find(|job| job.id == id) else {
            return Ok(false);
        };
        job.state = state.as_str().to_string();
        job.evidence = evidence.clone();
        job.updated_at = "now".to_string();
        if state.is_terminal() {
            job.finished_at = Some("now".to_string());
        }
        Ok(true)
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
