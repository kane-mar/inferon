//! Edge telemetry ingest (ITEM-052, hybrid data plane — slice 1).
//!
//! An on-prem edge node buffers traces locally and ships them here in batches.
//! The cloud stays authoritative for **cost and waste**: a batch is converted
//! into the same `TraceMessage`s the local pipeline produces and pushed through
//! the existing telemetry worker, so edge traffic shows up in the dashboard,
//! alerts, and billing identically. Inference payloads never traverse the cloud.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

use crate::{authenticate, json_error, telemetry::TraceMessage, AppState};

/// Largest accepted batch — the edge chunks its local buffer to fit.
pub const MAX_EDGE_BATCH: usize = 1000;

/// One trace as reported by an edge node. Cost and waste are **not** trusted
/// from the edge: the cloud recomputes both at flush time (prices and linter
/// rules can change centrally).
#[derive(Deserialize)]
pub struct EdgeTrace {
    /// The edge's trace id; the cloud mints one when absent.
    #[serde(default)]
    pub id: Option<Uuid>,
    pub model: String,
    #[serde(default)]
    pub system_prompt_hash: Option<String>,
    #[serde(default)]
    pub prompt_tokens: i32,
    #[serde(default)]
    pub completion_tokens: i32,
    #[serde(default)]
    pub cached_prompt_tokens: Option<i64>,
    #[serde(default)]
    pub ttft_ms: Option<i64>,
    pub total_duration_ms: i64,
    pub status_code: u16,
    #[serde(default)]
    pub raw_prompt: Option<Value>,
    #[serde(default)]
    pub raw_completion: Option<Value>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub guard_event: Option<Value>,
    #[serde(default)]
    pub routing_event: Option<Value>,
    #[serde(default)]
    pub router_event: Option<Value>,
    #[serde(default)]
    pub provider_id: Option<Uuid>,
    #[serde(default)]
    pub api_key_id: Option<Uuid>,
}

impl EdgeTrace {
    /// Convert to the pipeline's `TraceMessage`, tenant-scoped from the
    /// authenticated credential (never the body).
    pub fn into_message(self, tenant_id: Uuid) -> TraceMessage {
        TraceMessage {
            id: self.id.unwrap_or_else(Uuid::new_v4),
            tenant_id,
            request_model: self.model,
            system_prompt_hash: self.system_prompt_hash,
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            cached_prompt_tokens: self.cached_prompt_tokens,
            ttft_ms: self.ttft_ms,
            total_duration_ms: self.total_duration_ms,
            status_code: self.status_code,
            raw_prompt: self.raw_prompt,
            raw_completion: self.raw_completion,
            waste_detected: None,
            upstream_cost_usd: rust_decimal::Decimal::ZERO,
            cache_savings_usd: rust_decimal::Decimal::ZERO,
            truncated: self.truncated,
            guard_event: self.guard_event,
            routing_event: self.routing_event,
            router_event: self.router_event,
            provider_id: self.provider_id,
            api_key_id: self.api_key_id,
        }
    }
}

#[derive(Deserialize)]
pub struct EdgeBatch {
    pub traces: Vec<EdgeTrace>,
}

/// POST /v1/edge/traces — accept an edge node's trace batch.
///
/// Tenant-authenticated with the tenant's API key (`Authorization: Bearer`). The
/// batch is queued on the existing telemetry channel so the normal worker
/// enriches (cost, linter) and persists it; `accepted`/`dropped` report the
/// outcome under back-pressure.
pub async fn ingest(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(batch): Json<EdgeBatch>,
) -> Response {
    let tenant = match authenticate(&state, &headers).await {
        Ok(Some(tenant)) => tenant,
        Ok(None) => {
            return json_error(
                StatusCode::UNAUTHORIZED,
                "invalid_api_key",
                "missing or invalid Authorization header",
            )
        }
        Err(e) => {
            tracing::error!("edge ingest tenant lookup failed: {e}");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "tenant_store_error",
                "tenant lookup failed",
            );
        }
    };
    if batch.traces.len() > MAX_EDGE_BATCH {
        return json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "batch_too_large",
            "edge trace batch exceeds the limit",
        );
    }
    let mut accepted = 0usize;
    let mut dropped = 0usize;
    for trace in batch.traces {
        match state.telemetry_tx.try_send(trace.into_message(tenant.id)) {
            Ok(()) => accepted += 1,
            Err(_) => dropped += 1,
        }
    }
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "accepted": accepted, "dropped": dropped })),
    )
        .into_response()
}
