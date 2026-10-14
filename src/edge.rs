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
    /// ITEM-055: the edge's own outcome classification; the cloud accepts it but
    /// falls back to a status-derived value when absent/unknown.
    #[serde(default)]
    pub outcome: Option<String>,
    /// ITEM-057: the edge's own error class for a failed trace.
    #[serde(default)]
    pub error_class: Option<String>,
}

impl EdgeTrace {
    /// Convert to the pipeline's `TraceMessage`, tenant-scoped from the
    /// authenticated credential (never the body).
    pub(crate) fn into_message(self, tenant_id: Uuid) -> TraceMessage {
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
            outcome: crate::quality::Outcome::from_db(self.outcome.as_deref())
                .unwrap_or(if self.status_code >= 400 {
                    crate::quality::Outcome::Error
                } else {
                    crate::quality::Outcome::Ok
                }),
            error_class: crate::errors::ErrorClass::from_db(self.error_class.as_deref()),
            savings_usd: rust_decimal::Decimal::ZERO,
            savings_kind: None,
            cache_affinity: None,
            policy_version: None,
            proxy_ms: None,
        }
    }
}

pub type StoreResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// A fresh edge-node credential (shown once; only its hash is stored).
pub fn mint_edge_token() -> String {
    format!("inferon-edge-{}", Uuid::new_v4().simple())
}

/// A node as shown to the tenant — never carries the token.
#[derive(Clone, Debug)]
pub struct EdgeNode {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub last_seen_at: Option<String>,
    pub created_at: String,
}

/// Enrollment, node auth, and health (ITEM-065).
#[async_trait::async_trait]
pub trait EdgeNodeStore: Send + Sync {
    /// Enroll a node under `token_hash`; returns the node id.
    async fn enroll(&self, tenant_id: Uuid, name: &str, token_hash: &str) -> StoreResult<Uuid>;
    /// Resolve a node credential to `(node_id, tenant_id)`, or `None`.
    async fn authenticate(&self, token_hash: &str) -> StoreResult<Option<(Uuid, Uuid)>>;
    /// Record a heartbeat.
    async fn touch(&self, node_id: Uuid) -> StoreResult<()>;
    /// Delete a node (revocation); `false` when it is not this tenant's.
    async fn revoke(&self, tenant_id: Uuid, node_id: Uuid) -> StoreResult<bool>;
    async fn list(&self, tenant_id: Uuid) -> StoreResult<Vec<EdgeNode>>;
}

/// In-process double: (node, token_hash) pairs.
#[derive(Default)]
pub struct InMemoryEdgeNodeStore {
    rows: std::sync::Mutex<Vec<EdgeNode>>,
    hashes: std::sync::Mutex<std::collections::HashMap<String, Uuid>>,
}

#[async_trait::async_trait]
impl EdgeNodeStore for InMemoryEdgeNodeStore {
    async fn enroll(&self, tenant_id: Uuid, name: &str, token_hash: &str) -> StoreResult<Uuid> {
        let id = Uuid::new_v4();
        self.rows.lock().unwrap().push(EdgeNode {
            id,
            tenant_id,
            name: name.to_string(),
            last_seen_at: None,
            created_at: String::new(),
        });
        self.hashes.lock().unwrap().insert(token_hash.to_string(), id);
        Ok(id)
    }

    async fn authenticate(&self, token_hash: &str) -> StoreResult<Option<(Uuid, Uuid)>> {
        let Some(id) = self.hashes.lock().unwrap().get(token_hash).copied() else {
            return Ok(None);
        };
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .find(|node| node.id == id)
            .map(|node| (node.id, node.tenant_id)))
    }

    async fn touch(&self, node_id: Uuid) -> StoreResult<()> {
        if let Some(node) = self.rows.lock().unwrap().iter_mut().find(|node| node.id == node_id) {
            node.last_seen_at = Some("now".to_string());
        }
        Ok(())
    }

    async fn revoke(&self, tenant_id: Uuid, node_id: Uuid) -> StoreResult<bool> {
        let mut rows = self.rows.lock().unwrap();
        let before = rows.len();
        rows.retain(|node| !(node.id == node_id && node.tenant_id == tenant_id));
        let removed = rows.len() != before;
        if removed {
            self.hashes.lock().unwrap().retain(|_, id| *id != node_id);
        }
        Ok(removed)
    }

    async fn list(&self, tenant_id: Uuid) -> StoreResult<Vec<EdgeNode>> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .filter(|node| node.tenant_id == tenant_id)
            .cloned()
            .collect())
    }
}

/// Postgres-backed edge nodes (the production store).
pub struct PostgresEdgeNodeStore {
    pool: sqlx::PgPool,
}

impl PostgresEdgeNodeStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl EdgeNodeStore for PostgresEdgeNodeStore {
    async fn enroll(&self, tenant_id: Uuid, name: &str, token_hash: &str) -> StoreResult<Uuid> {
        let (id,): (Uuid,) = sqlx::query_as(
            "INSERT INTO edge_nodes (tenant_id, name, token_hash) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(tenant_id)
        .bind(name)
        .bind(token_hash)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    async fn authenticate(&self, token_hash: &str) -> StoreResult<Option<(Uuid, Uuid)>> {
        let row: Option<(Uuid, Uuid)> =
            sqlx::query_as("SELECT id, tenant_id FROM edge_nodes WHERE token_hash = $1")
                .bind(token_hash)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row)
    }

    async fn touch(&self, node_id: Uuid) -> StoreResult<()> {
        sqlx::query("UPDATE edge_nodes SET last_seen_at = NOW() WHERE id = $1")
            .bind(node_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn revoke(&self, tenant_id: Uuid, node_id: Uuid) -> StoreResult<bool> {
        let result = sqlx::query("DELETE FROM edge_nodes WHERE id = $1 AND tenant_id = $2")
            .bind(node_id)
            .bind(tenant_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn list(&self, tenant_id: Uuid) -> StoreResult<Vec<EdgeNode>> {
        let rows: Vec<(Uuid, Uuid, String, Option<String>, String)> = sqlx::query_as(
            "SELECT id, tenant_id, name, last_seen_at::text, created_at::text
             FROM edge_nodes WHERE tenant_id = $1 ORDER BY created_at",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, tenant_id, name, last_seen_at, created_at)| EdgeNode {
                id,
                tenant_id,
                name,
                last_seen_at,
                created_at,
            })
            .collect())
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
    let _in_flight = crate::metrics::request_started();
    let (tenant_id, node_id) = match edge_node_auth(&state, &headers).await {
        Ok(Some((node_id, tenant_id))) => (tenant_id, Some(node_id)),
        Ok(None) => match authenticate(&state, &headers).await {
            Ok(Some(tenant)) => (tenant.id, None),
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
        },
        Err(e) => {
            tracing::error!("edge node lookup failed: {e}");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "edge_store_error",
                "edge node lookup failed",
            );
        }
    };
    if let Some(node_id) = node_id {
        let _ = state.edge_store.touch(node_id).await;
    }
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
        match state.telemetry_tx.try_send(trace.into_message(tenant_id)) {
            Ok(()) => {
                crate::metrics::count_queued();
                accepted += 1;
            }
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                crate::metrics::count_dropped(true);
                dropped += 1;
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                crate::metrics::count_dropped(false);
                dropped += 1;
            }
        }
    }
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "accepted": accepted, "dropped": dropped })),
    )
        .into_response()
}

/// Resolve an edge-node credential (`Authorization: Bearer <edge token>`).
pub async fn edge_node_auth(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> StoreResult<Option<(Uuid, Uuid)>> {
    let Some(key) = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return Ok(None);
    };
    if key.is_empty() {
        return Ok(None);
    }
    state.edge_store.authenticate(&crate::sha256_hex(key)).await
}

/// GET /v1/edge/config — the tenant's versioned config snapshot for an enrolled
/// node (ITEM-065 AC2). The provider key never appears here: it stays local.
pub async fn config(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (node_id, tenant_id) = match edge_node_auth(&state, &headers).await {
        Ok(Some(pair)) => pair,
        Ok(None) => {
            return json_error(
                StatusCode::UNAUTHORIZED,
                "invalid_edge_token",
                "missing or invalid edge credential",
            )
        }
        Err(e) => {
            tracing::error!("edge node lookup failed: {e}");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "edge_store_error",
                "edge node lookup failed",
            );
        }
    };
    let _ = state.edge_store.touch(node_id).await;
    let snapshot = match build_config(&state, tenant_id).await {
        Ok(snapshot) => snapshot,
        Err(e) => {
            tracing::error!("edge config build failed: {e}");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "config_error",
                "could not build the config snapshot",
            );
        }
    };
    let version = crate::sha256_hex(&snapshot.to_string());
    (
        StatusCode::OK,
        Json(serde_json::json!({ "version": version, "config": snapshot })),
    )
        .into_response()
}

/// The non-secret config a node needs to serve locally: client key hashes (so
/// local auth matches the cloud), routing/budget/guard controls, the active
/// policy, and the advertised model ids.
async fn build_config(state: &AppState, tenant_id: Uuid) -> Result<Value, String> {
    let key_hashes: Vec<String> = sqlx::query_scalar(
        "SELECT key_hash FROM api_keys WHERE tenant_id = $1 AND revoked_at IS NULL",
    )
    .bind(tenant_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    let config: Option<(i64, i64, Option<String>, bool, bool, f64)> = sqlx::query_as(
        "SELECT budget_limit_cents, current_spend_cents, cheap_fallback_model, guard_enabled, router_enabled, router_threshold
         FROM tenant_configurations WHERE tenant_id = $1",
    )
    .bind(tenant_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    let fallbacks = state.routing_store.fallbacks(tenant_id).await.unwrap_or_default();
    let policy = state.policy_store.active(tenant_id).await.ok().flatten();
    let models: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT pm.model_id FROM provider_models pm
         JOIN providers p ON p.id = pm.provider_id WHERE p.tenant_id = $1",
    )
    .bind(tenant_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    Ok(serde_json::json!({
        "tenant_id": tenant_id.to_string(),
        "key_hashes": key_hashes,
        "fallbacks": fallbacks,
        "models": models,
        "policy": policy,
        "budget": config.map(
            |(limit_cents, spent_cents, cheap_fallback_model, guard_enabled, router_enabled, router_threshold)| {
                serde_json::json!({
                    "limit_cents": limit_cents,
                    "spent_cents": spent_cents,
                    "cheap_fallback_model": cheap_fallback_model,
                    "guard_enabled": guard_enabled,
                    "router_enabled": router_enabled,
                    "router_threshold": router_threshold,
                })
            },
        ),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn enroll_authenticate_touch_and_revoke() {
        let store = InMemoryEdgeNodeStore::default();
        let tenant = Uuid::new_v4();
        let token = mint_edge_token();
        let hash = crate::sha256_hex(&token);
        let id = store.enroll(tenant, "node-1", &hash).await.unwrap();
        assert_eq!(store.authenticate(&hash).await.unwrap(), Some((id, tenant)));
        store.touch(id).await.unwrap();
        let nodes = store.list(tenant).await.unwrap();
        assert_eq!(nodes.len(), 1);
        assert!(nodes[0].last_seen_at.is_some());
        // Revoking stops the credential immediately.
        assert!(store.revoke(tenant, id).await.unwrap());
        assert_eq!(store.authenticate(&hash).await.unwrap(), None);
        assert!(store.list(tenant).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_node_cannot_be_revoked_by_another_tenant() {
        let store = InMemoryEdgeNodeStore::default();
        let tenant = Uuid::new_v4();
        let id = store.enroll(tenant, "n", "hash").await.unwrap();
        assert!(!store.revoke(Uuid::new_v4(), id).await.unwrap(), "wrong tenant");
        assert_eq!(store.authenticate("hash").await.unwrap(), Some((id, tenant)));
    }
}
