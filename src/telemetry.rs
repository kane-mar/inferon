//! Telemetry pipeline (ITEM-005/006): batched, off-the-hot-path persistence of
//! completed requests. Handlers only enqueue a `TraceMessage`; this module's
//! worker batches messages and flushes them through the `TraceSink` seam
//! (linter + cost enrichment happens here, never on the request path).

use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;
use sqlx::types::Json;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{cost, linter};

#[derive(Clone, Debug)]
pub struct TraceMessage {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub request_model: String,
    pub system_prompt_hash: Option<String>,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    /// Provider-reported prompt tokens served from cache
    /// (`usage.prompt_tokens_details.cached_tokens`), when the provider sends it.
    pub cached_prompt_tokens: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub total_duration_ms: i64,
    pub status_code: u16,
    pub raw_prompt: Option<Value>,
    pub raw_completion: Option<Value>,
    pub waste_detected: Option<Value>,
    pub upstream_cost_usd: rust_decimal::Decimal,
    /// True when a streaming response ended before the upstream finished
    /// (client abort or mid-stream error) — token counts are then best-effort.
    pub truncated: bool,
    /// Schema-guard event (ITEM-025): present only when the guard fired on a
    /// guarded request (cheap tier + schema contract). Unguarded traffic keeps
    /// this None — the telemetry is additive only (ITEM-026 AC4).
    pub guard_event: Option<Value>,
}

/// Flush seam for the telemetry worker: batches of TraceMessages are handed
/// to a sink exactly once per flush (one DB transaction for PostgresTraceSink).
#[async_trait::async_trait]
pub trait TraceSink: Send {
    async fn flush(&mut self, batch: &mut Vec<TraceMessage>);
}

pub async fn load_system_prompt_counts(pool: &sqlx::PgPool) -> Result<HashMap<String, u32>, sqlx::Error> {
    let rows: Vec<(String, i32)> =
        sqlx::query_as("SELECT hash, count FROM system_prompt_counts")
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(hash, count)| (hash, count.max(0) as u32)).collect())
}

pub struct PostgresTraceSink {
    pool: sqlx::PgPool,
}

impl PostgresTraceSink {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl TraceSink for PostgresTraceSink {
    async fn flush(&mut self, batch: &mut Vec<TraceMessage>) {
        let mut tx = match self.pool.begin().await {
            Ok(tx) => tx,
            Err(e) => {
                tracing::error!("telemetry: cannot begin transaction, dropping {} traces: {e}", batch.len());
                batch.clear();
                return;
            }
        };
        for msg in batch.drain(..) {
            let result = sqlx::query(
                r#"INSERT INTO traces (
                    id, tenant_id, request_model, system_prompt_hash,
                    prompt_tokens, completion_tokens, time_to_first_token_ms,
                    total_duration_ms, status_code, raw_prompt, raw_completion,
                    upstream_cost_usd, waste_detected, truncated, guard_event
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)"#,
            )
            .bind(msg.id)
            .bind(msg.tenant_id)
            .bind(&msg.request_model)
            .bind(msg.system_prompt_hash.as_deref())
            .bind(msg.prompt_tokens)
            .bind(msg.completion_tokens)
            .bind(msg.ttft_ms.map(|t| t as i32))
            .bind(msg.total_duration_ms as i32)
            .bind(i32::from(msg.status_code))
            .bind(msg.raw_prompt.as_ref().map(Json))
            .bind(msg.raw_completion.as_ref().map(Json))
            .bind(msg.upstream_cost_usd)
            .bind(msg.waste_detected.as_ref().map(Json))
            .bind(msg.truncated)
            .bind(msg.guard_event.as_ref().map(Json))
            .execute(&mut *tx)
            .await;
            if let Err(e) = result {
                tracing::error!("telemetry: trace insert failed, rolling back batch: {e}");
                return; // tx dropped -> rollback keeps the flush atomic (AC6)
            }
            // Persist the per-hash occurrence count (ITEM-010) in the same
            // transaction, so rule B repeat counts survive restarts.
            if let Some(hash) = &msg.system_prompt_hash {
                let count_result = sqlx::query(
                    "INSERT INTO system_prompt_counts (hash, count) VALUES ($1, 1)
                     ON CONFLICT (hash) DO UPDATE SET count = system_prompt_counts.count + 1",
                )
                .bind(hash)
                .execute(&mut *tx)
                .await;
                if let Err(e) = count_result {
                    tracing::error!("telemetry: count upsert failed, rolling back batch: {e}");
                    return;
                }
            }
            // Accrue actual spend toward the tenant budget (ITEM-016), same
            // transaction. No-op when the tenant has no configuration row.
            let cents = cost::cost_to_cents(msg.upstream_cost_usd);
            if cents > 0 {
                let spend_result = sqlx::query(
                    "UPDATE tenant_configurations
                     SET current_spend_cents = current_spend_cents + $1
                     WHERE tenant_id = $2",
                )
                .bind(cents)
                .bind(msg.tenant_id)
                .execute(&mut *tx)
                .await;
                if let Err(e) = spend_result {
                    tracing::error!("telemetry: spend accrual failed, rolling back batch: {e}");
                    return;
                }
            }
        }
        if let Err(e) = tx.commit().await {
            tracing::error!("telemetry: commit failed: {e}");
        }
    }
}

/// Background worker: batches TraceMessages and flushes them to the sink every
/// `flush_interval` or every `batch_size` items, whichever comes first. DB I/O
/// happens only here, never on the hot request path (AC2).
/// `initial_counts` seeds the per-hash repeat counter from persisted state so
/// linter rule B keeps counting across restarts (ITEM-010).
pub async fn telemetry_worker(
    mut rx: mpsc::Receiver<TraceMessage>,
    mut sink: Box<dyn TraceSink>,
    flush_interval: Duration,
    batch_size: usize,
    initial_counts: HashMap<String, u32>,
) {
    let mut buffer: Vec<TraceMessage> = Vec::with_capacity(batch_size);
    let mut interval = tokio::time::interval(flush_interval);
    let mut hash_counts: HashMap<String, u32> = initial_counts;
    loop {
        tokio::select! {
            msg = rx.recv() => match msg {
                Some(mut msg) => {
                    let repeat_count = match &msg.system_prompt_hash {
                        Some(hash) => {
                            let count = hash_counts.entry(hash.clone()).or_insert(0);
                            *count += 1;
                            *count
                        }
                        None => 0,
                    };
                    // Enrich off the hot path (ITEM-006): lint + cost. An
                    // unlisted model is costed at the default row — surface it
                    // once instead of silently over/under-charging.
                    cost::warn_if_unpriced(&msg.request_model);
                    msg.waste_detected = linter::lint(&msg, repeat_count);
                    msg.upstream_cost_usd = cost::calculate_cost(
                        &msg.request_model,
                        msg.prompt_tokens,
                        msg.completion_tokens,
                        msg.cached_prompt_tokens.unwrap_or(0),
                    );
                    buffer.push(msg);
                    if buffer.len() >= batch_size {
                        sink.flush(&mut buffer).await;
                    }
                }
                None => {
                    if !buffer.is_empty() {
                        sink.flush(&mut buffer).await;
                    }
                    return;
                }
            },
            _ = interval.tick() => {
                if !buffer.is_empty() {
                    sink.flush(&mut buffer).await;
                }
            }
        }
    }
}
