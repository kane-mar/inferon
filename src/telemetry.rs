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
    /// ITEM-085: the captured completion hit the size cap (a head, not a pair).
    pub raw_completion_capped: bool,
    pub waste_detected: Option<Value>,
    pub upstream_cost_usd: rust_decimal::Decimal,
    /// ITEM-053: estimated saving vs a no-cache call (the provider's prompt
    /// cache served part of the prompt), computed at telemetry time.
    pub cache_savings_usd: rust_decimal::Decimal,
    /// True when a streaming response ended before the upstream finished
    /// (client abort or mid-stream error) — token counts are then best-effort.
    pub truncated: bool,
    /// Schema-guard event (ITEM-025): present only when the guard fired on a
    /// guarded request (cheap tier + schema contract). Unguarded traffic keeps
    /// this None — the telemetry is additive only (ITEM-026 AC4).
    pub guard_event: Option<Value>,
    /// ITEM-045: set when a fallback chain changed the serving model.
    pub routing_event: Option<Value>,
    /// ITEM-054: set when the heuristic router downshifted the model.
    pub router_event: Option<Value>,
    /// ITEM-047: the provider that served the request (`None` = shared key).
    pub provider_id: Option<Uuid>,
    /// ITEM-077: served by a locally hosted provider — costed at zero.
    pub is_local: bool,
    /// ITEM-047: the API key that authenticated the request (`None` = legacy).
    pub api_key_id: Option<Uuid>,
    /// ITEM-055: whether the response was usable, derived from the status +
    /// body (never the client).
    pub outcome: crate::quality::Outcome,
    /// ITEM-057: why a failed/aborted request failed (`None` on success).
    pub error_class: Option<crate::errors::ErrorClass>,
    /// ITEM-058: estimated saving from serving a cheaper model than the client
    /// asked for (a router downgrade or a fallback).
    pub savings_usd: rust_decimal::Decimal,
    /// ITEM-058: what produced `savings_usd` ("router" | "fallback").
    pub savings_kind: Option<String>,
    /// ITEM-063: set when cache affinity changed the serving model.
    pub cache_affinity: Option<Value>,
    /// ITEM-064: the adaptive policy version in force for this request.
    pub policy_version: Option<i64>,
    /// ITEM-066: the proxy's own pre-upstream time (request-received → dispatch).
    pub proxy_ms: Option<i64>,
    /// ITEM-092: the same overhead in microseconds (the performance contract).
    pub proxy_us: Option<i64>,
    /// ITEM-086: dataset qualification, decided at write time. `None` when no
    /// completion was captured (there is no pair to qualify or exclude).
    pub example_state: Option<&'static str>,
    pub example_reason: Option<&'static str>,
    /// ITEM-086: stable identity of the captured completion (exact dedupe).
    pub completion_hash: Option<String>,
    /// ITEM-093: the client's transaction id, when supplied.
    pub session_id: Option<String>,
    /// ITEM-090: pre-flight payload findings (None = nothing fired).
    pub request_lint: Option<Value>,
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

    /// Best-effort write of the warm-prefix targets (ITEM-063). Runs outside the
    /// batch transaction so an affinity failure never rolls back traces.
    async fn record_affinity(&self, hits: Vec<(Uuid, String, Option<Uuid>, String)>) {
        for (tenant_id, hash, provider_id, model) in hits {
            let result = sqlx::query(
                "INSERT INTO prompt_affinity (tenant_id, system_prompt_hash, provider_id, model, last_hit_at)
                 VALUES ($1, $2, $3, $4, NOW())
                 ON CONFLICT (tenant_id, system_prompt_hash) DO UPDATE
                   SET provider_id = EXCLUDED.provider_id, model = EXCLUDED.model, last_hit_at = NOW()",
            )
            .bind(tenant_id)
            .bind(hash)
            .bind(provider_id)
            .bind(model)
            .execute(&self.pool)
            .await;
            if let Err(e) = result {
                tracing::warn!("prompt affinity upsert failed: {e}");
            }
        }
    }
}

/// The trace insert. Read as data by `tests` — a column the migrations do not
/// create, or one listed twice, fails the test suite instead of the deploy.
const TRACE_INSERT: &str = r#"INSERT INTO traces (
                    id, tenant_id, request_model, system_prompt_hash,
                    prompt_tokens, completion_tokens, time_to_first_token_ms,
                    total_duration_ms, status_code, raw_prompt, raw_completion, raw_completion_capped,
                    upstream_cost_usd, waste_detected, truncated, guard_event, routing_event,
                    provider_id, api_key_id, cached_prompt_tokens, cache_savings_usd, router_event,
                    outcome, error_class, savings_usd, savings_kind, cache_affinity, policy_version,
                    proxy_ms, proxy_us,
                    example_state, example_reason, completion_hash, session_id,
                    request_lint
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31, $32, $33, $34, $35)"#;

#[async_trait::async_trait]
impl TraceSink for PostgresTraceSink {
    async fn flush(&mut self, batch: &mut Vec<TraceMessage>) {
        let total = batch.len();
        let mut tx = match self.pool.begin().await {
            Ok(tx) => tx,
            Err(e) => {
                tracing::error!("telemetry: cannot begin transaction, dropping {} traces: {e}", batch.len());
                batch.clear();
                return;
            }
        };
        let mut affinity_hits: Vec<(Uuid, String, Option<Uuid>, String)> = Vec::new();
        for msg in batch.drain(..) {
            let result = sqlx::query(TRACE_INSERT)
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
                .bind(msg.raw_completion_capped)
                .bind(msg.upstream_cost_usd)
                .bind(msg.waste_detected.as_ref().map(Json))
                .bind(msg.truncated)
                .bind(msg.guard_event.as_ref().map(Json))
                .bind(msg.routing_event.as_ref().map(Json))
                .bind(msg.provider_id)
                .bind(msg.api_key_id)
                .bind(msg.cached_prompt_tokens.map(|t| t as i32))
                .bind(msg.cache_savings_usd)
                .bind(msg.router_event.as_ref().map(Json))
                .bind(msg.outcome.as_str())
                .bind(msg.error_class.map(|class| class.as_str()))
                .bind(msg.savings_usd)
                .bind(msg.savings_kind.as_deref())
                .bind(msg.cache_affinity.as_ref().map(Json))
                .bind(msg.policy_version)
                .bind(msg.proxy_ms.map(|t| t as i32))
                .bind(msg.proxy_us)
                .bind(msg.example_state)
                .bind(msg.example_reason)
                .bind(msg.completion_hash.as_deref())
                .bind(msg.session_id.as_deref())
                .bind(msg.request_lint.as_ref().map(Json))
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
            // ITEM-063: when the provider served this prefix from cache, remember
            // it as the warm target for the prefix (best-effort, after commit).
            if let (Some(hash), Some(cached)) = (&msg.system_prompt_hash, msg.cached_prompt_tokens) {
                if cached > 0 {
                    affinity_hits.push((
                        msg.tenant_id,
                        hash.clone(),
                        msg.provider_id,
                        msg.request_model.clone(),
                    ));
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
        } else {
            crate::metrics::count_persisted(total);
            crate::metrics::record_flush();
            self.record_affinity(affinity_hits).await;
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
                    // A locally hosted model is deliberately free — not an
                    // unknown model, so it must not hit the unpriced default or
                    // the unpriced counter (ITEM-077).
                    if !msg.is_local {
                        cost::warn_if_unpriced(&msg.request_model);
                    }
                    msg.waste_detected = linter::lint(&msg, repeat_count);
                    // ITEM-086: qualify the captured pair for the dataset surface.
                    if let Some(classification) = crate::datasets::classify(&msg) {
                        msg.example_state = Some(classification.state);
                        msg.example_reason = classification.reason;
                        msg.completion_hash = classification.completion_hash;
                    }
                    // ITEM-053: cost with caching, and the saving vs no cache.
                    let cached = msg.cached_prompt_tokens.unwrap_or(0);
                    msg.cache_savings_usd = cost::trace_cache_savings(
                        &msg.request_model,
                        msg.prompt_tokens,
                        msg.completion_tokens,
                        cached,
                        msg.is_local,
                    );
                    msg.upstream_cost_usd = cost::trace_cost(
                        &msg.request_model,
                        msg.prompt_tokens,
                        msg.completion_tokens,
                        cached,
                        msg.is_local,
                    );
                    // ITEM-058: what serving a cheaper model saved vs the model
                    // the client asked for (router downgrade or fallback).
                    let counterfactual = msg
                        .router_event
                        .as_ref()
                        .or(msg.routing_event.as_ref())
                        .and_then(|event| event["from_model"].as_str());
                    if let Some(asked) = counterfactual {
                        msg.savings_usd = cost::trace_savings(
                            asked,
                            &msg.request_model,
                            msg.prompt_tokens,
                            msg.completion_tokens,
                            cached,
                            msg.is_local,
                        );
                        msg.savings_kind = Some(
                            if msg.router_event.is_some() { "router" } else { "fallback" }
                                .to_string(),
                        );
                    }
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

#[cfg(test)]
mod tests;
