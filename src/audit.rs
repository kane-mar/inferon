// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! Read-only cost-audit shares.
//!
//! A tenant mints a high-entropy token and can hand the audit report to someone
//! without an account. Only the token's hash is stored; the report is built from
//! aggregates, so no prompts, keys or completions are exposed.
//!
//! The public report is a deliberately narrow **read model**: the dashboard can
//! evolve, the shared link keeps a stable shape.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::Mutex;
use uuid::Uuid;

use crate::AppState;
use crate::cost;

/// Prefix so a leaked string is recognisable as a share credential.
pub const TOKEN_PREFIX: &str = "inferon-audit-";

pub fn mint_token() -> String {
    format!("{TOKEN_PREFIX}{}", Uuid::new_v4().simple())
}

/// A share as shown to its owner — never the token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditShare {
    pub id: Uuid,
    pub label: String,
    pub created_at: String,
    pub last_viewed_at: Option<String>,
    pub revoked_at: Option<String>,
}

#[async_trait::async_trait]
pub trait AuditShareStore: Send + Sync {
    async fn create(
        &self,
        tenant_id: Uuid,
        label: &str,
        token_hash: &str,
    ) -> Result<Uuid, String>;
    async fn list(&self, tenant_id: Uuid) -> Result<Vec<AuditShare>, String>;
    async fn revoke(&self, tenant_id: Uuid, id: Uuid) -> Result<bool, String>;
    /// The tenant a live token belongs to, recording this view.
    async fn resolve(&self, token_hash: &str) -> Result<Option<Uuid>, String>;
}

pub struct PostgresAuditShareStore {
    pool: PgPool,
}

impl PostgresAuditShareStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl AuditShareStore for PostgresAuditShareStore {
    async fn create(
        &self,
        tenant_id: Uuid,
        label: &str,
        token_hash: &str,
    ) -> Result<Uuid, String> {
        let (id,): (Uuid,) = sqlx::query_as(
            "INSERT INTO audit_shares (tenant_id, label, token_hash)
             VALUES ($1, $2, $3)
             RETURNING id",
        )
        .bind(tenant_id)
        .bind(label)
        .bind(token_hash)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(id)
    }

    async fn list(&self, tenant_id: Uuid) -> Result<Vec<AuditShare>, String> {
        let rows: Vec<(Uuid, String, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT id, label, created_at::text, last_viewed_at::text, revoked_at::text
               FROM audit_shares
              WHERE tenant_id = $1
              ORDER BY created_at DESC",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|(id, label, created_at, last_viewed_at, revoked_at)| AuditShare {
                id,
                label,
                created_at,
                last_viewed_at,
                revoked_at,
            })
            .collect())
    }

    async fn revoke(&self, tenant_id: Uuid, id: Uuid) -> Result<bool, String> {
        let result = sqlx::query(
            "UPDATE audit_shares
                SET revoked_at = NOW()
              WHERE id = $1 AND tenant_id = $2 AND revoked_at IS NULL",
        )
        .bind(id)
        .bind(tenant_id)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(result.rows_affected() > 0)
    }

    async fn resolve(&self, token_hash: &str) -> Result<Option<Uuid>, String> {
        let row: Option<(Uuid,)> = sqlx::query_as(
            "UPDATE audit_shares
                SET last_viewed_at = NOW()
              WHERE token_hash = $1 AND revoked_at IS NULL
              RETURNING tenant_id",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(row.map(|(tenant_id,)| tenant_id))
    }
}

/// The test double.
#[derive(Default)]
pub struct InMemoryAuditShareStore {
    shares: Mutex<Vec<(AuditShare, Uuid, String)>>,
}

#[async_trait::async_trait]
impl AuditShareStore for InMemoryAuditShareStore {
    async fn create(
        &self,
        tenant_id: Uuid,
        label: &str,
        token_hash: &str,
    ) -> Result<Uuid, String> {
        let id = Uuid::new_v4();
        self.shares.lock().unwrap().push((
            AuditShare {
                id,
                label: label.to_string(),
                created_at: "now".to_string(),
                last_viewed_at: None,
                revoked_at: None,
            },
            tenant_id,
            token_hash.to_string(),
        ));
        Ok(id)
    }

    async fn list(&self, tenant_id: Uuid) -> Result<Vec<AuditShare>, String> {
        Ok(self
            .shares
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, owner, _)| *owner == tenant_id)
            .map(|(share, _, _)| share.clone())
            .collect())
    }

    async fn revoke(&self, tenant_id: Uuid, id: Uuid) -> Result<bool, String> {
        let mut shares = self.shares.lock().unwrap();
        let Some((share, _, _)) = shares
            .iter_mut()
            .find(|(share, owner, _)| share.id == id && *owner == tenant_id)
        else {
            return Ok(false);
        };
        if share.revoked_at.is_some() {
            return Ok(false);
        }
        share.revoked_at = Some("now".to_string());
        Ok(true)
    }

    async fn resolve(&self, token_hash: &str) -> Result<Option<Uuid>, String> {
        let mut shares = self.shares.lock().unwrap();
        let Some((share, owner, _)) = shares
            .iter_mut()
            .find(|(share, _, hash)| hash == token_hash && share.revoked_at.is_none())
        else {
            return Ok(None);
        };
        share.last_viewed_at = Some("now".to_string());
        Ok(Some(*owner))
    }
}

/// `(requests, errors, cost, savings, router, fallback, cache, truncated,
/// affinity)` — the report's headline aggregate.
type AuditSummary = (i64, i64, Decimal, Decimal, Decimal, Decimal, Decimal, i64, i64);

/// `(flag, model, suggested, requests, prompt, completion, spend)` from the
/// batch linter, grouped so each row's own suggestion prices the saving.
type WasteRow = (String, String, String, i64, i64, i64, Decimal);

const WINDOW_FILTER: &str = "created_at >= NOW() - ($2 * INTERVAL '1 day')";

async fn summary(pool: &PgPool, tenant_id: Uuid, days: i64) -> Result<AuditSummary, String> {
    sqlx::query_as(&format!(
        "SELECT count(*)::bigint,
                count(*) FILTER (WHERE status_code >= 400)::bigint,
                COALESCE(SUM(upstream_cost_usd), 0),
                COALESCE(SUM(savings_usd), 0),
                COALESCE(SUM(savings_usd) FILTER (WHERE savings_kind = 'router'), 0),
                COALESCE(SUM(savings_usd) FILTER (WHERE savings_kind = 'fallback'), 0),
                COALESCE(SUM(cache_savings_usd), 0),
                count(*) FILTER (WHERE truncated)::bigint,
                count(*) FILTER (WHERE cache_affinity IS NOT NULL)::bigint
           FROM traces
          WHERE tenant_id = $1 AND {WINDOW_FILTER}"
    ))
    .bind(tenant_id)
    .bind(days)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())
}

async fn waste_rows(
    pool: &PgPool,
    tenant_id: Uuid,
    days: i64,
) -> Result<Vec<WasteRow>, String> {
    sqlx::query_as(&format!(
        "SELECT flag, request_model,
                COALESCE(waste_detected->>'suggested_model', '') AS suggested,
                count(*)::bigint, COALESCE(SUM(prompt_tokens), 0)::bigint,
                COALESCE(SUM(completion_tokens), 0)::bigint,
                COALESCE(SUM(upstream_cost_usd), 0)
           FROM traces
           CROSS JOIN LATERAL (VALUES
              ('over_provisioned', waste_detected->>'over_provisioned'),
              ('model_misuse', waste_detected->>'model_misuse'),
              ('cache_miss', waste_detected->>'cache_miss_system_prompt')) AS f(flag, fired)
          WHERE tenant_id = $1 AND {WINDOW_FILTER} AND f.fired = 'true'
          GROUP BY flag, request_model, suggested"
    ))
    .bind(tenant_id)
    .bind(days)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())
}

/// Aggregate the grouped waste rows into the six-finding shape. A cache miss
/// and an empty suggestion are spend-only: there is no cheaper model to price.
fn waste_json(rows: Vec<WasteRow>) -> Value {
    let mut waste = serde_json::Map::new();
    for (flag, model, suggested, requests, prompt, completion, spend) in rows {
        let savings = if flag == "cache_miss" || suggested.is_empty() {
            Decimal::ZERO
        } else {
            crate::cost::savings_vs(
                &model,
                &suggested,
                cost::tokens_as_i32(prompt),
                cost::tokens_as_i32(completion),
                0,
            )
        };
        let entry = waste
            .entry(flag)
            .or_insert_with(|| json!({"requests": 0, "spend_usd": "0", "savings_est_usd": "0"}));
        let prior_requests = entry["requests"].as_i64().unwrap_or(0);
        let prior_spend = decimal_from(entry["spend_usd"].as_str());
        let prior_savings = decimal_from(entry["savings_est_usd"].as_str());
        entry["requests"] = json!(prior_requests + requests);
        entry["spend_usd"] = json!((prior_spend + spend).to_string());
        entry["savings_est_usd"] = json!((prior_savings + savings).to_string());
    }
    Value::Object(waste)
}

async fn top_model(
    pool: &PgPool,
    tenant_id: Uuid,
    days: i64,
) -> Result<Option<(String, Decimal)>, String> {
    sqlx::query_as(&format!(
        "SELECT request_model, COALESCE(SUM(upstream_cost_usd), 0)
           FROM traces
          WHERE tenant_id = $1 AND {WINDOW_FILTER}
          GROUP BY 1 ORDER BY 2 DESC LIMIT 1"
    ))
    .bind(tenant_id)
    .bind(days)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())
}

async fn top_provider(
    pool: &PgPool,
    tenant_id: Uuid,
    days: i64,
) -> Result<Option<(String, Decimal)>, String> {
    sqlx::query_as(&format!(
        "SELECT COALESCE(p.name, 'Shared platform key'), COALESCE(SUM(t.upstream_cost_usd), 0)
           FROM traces t LEFT JOIN providers p ON p.id = t.provider_id
          WHERE t.tenant_id = $1 AND t.{WINDOW_FILTER}
          GROUP BY 1 ORDER BY 2 DESC LIMIT 1"
    ))
    .bind(tenant_id)
    .bind(days)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())
}

async fn top_key(
    pool: &PgPool,
    tenant_id: Uuid,
    days: i64,
) -> Result<Option<(String, Decimal)>, String> {
    sqlx::query_as(&format!(
        "SELECT COALESCE(k.label, 'Unknown key'), COALESCE(SUM(t.upstream_cost_usd), 0)
           FROM traces t LEFT JOIN api_keys k ON k.id = t.api_key_id
          WHERE t.tenant_id = $1 AND t.{WINDOW_FILTER}
          GROUP BY 1 ORDER BY 2 DESC LIMIT 1"
    ))
    .bind(tenant_id)
    .bind(days)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())
}

async fn quality(
    pool: &PgPool,
    tenant_id: Uuid,
    days: i64,
) -> Result<(i64, i64, i64, i64), String> {
    sqlx::query_as(&format!(
        "SELECT count(*)::bigint,
                count(*) FILTER (WHERE status_code >= 400)::bigint,
                count(*) FILTER (WHERE truncated)::bigint,
                count(*) FILTER (
                    WHERE guard_event->>'outcome' IN ('fallback', 'fallback_failed')
                )::bigint
           FROM traces
          WHERE tenant_id = $1 AND {WINDOW_FILTER}"
    ))
    .bind(tenant_id)
    .bind(days)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())
}

async fn errors(
    pool: &PgPool,
    tenant_id: Uuid,
    days: i64,
) -> Result<Vec<(String, i64)>, String> {
    sqlx::query_as(&format!(
        "SELECT error_class, count(*)::bigint
           FROM traces
          WHERE tenant_id = $1 AND {WINDOW_FILTER} AND error_class IS NOT NULL
          GROUP BY 1 ORDER BY 2 DESC"
    ))
    .bind(tenant_id)
    .bind(days)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())
}

/// The six-finding read model behind a shared link. Aggregates only: no prompt,
/// key or completion ever leaves this function.
pub async fn report(pool: &PgPool, tenant_id: Uuid, days: i64) -> Result<Value, String> {
    let summary = summary(pool, tenant_id, days).await?;
    let waste = waste_json(waste_rows(pool, tenant_id, days).await?);
    let top = json!({
        "model": top_model(pool, tenant_id, days).await?.map(|(name, cost)| json!({"name": name, "cost_usd": cost.to_string()})),
        "provider": top_provider(pool, tenant_id, days).await?.map(|(name, cost)| json!({"name": name, "cost_usd": cost.to_string()})),
        "key": top_key(pool, tenant_id, days).await?.map(|(name, cost)| json!({"name": name, "cost_usd": cost.to_string()})),
    });
    let quality = quality(pool, tenant_id, days).await?;
    let errors = errors(pool, tenant_id, days).await?;
    let rate = |part: i64, total: i64| -> Option<f64> {
        (total > 0).then(|| part as f64 / total as f64)
    };

    Ok(json!({
        "window_days": days,
        "summary": {
            "requests": summary.0,
            "errors": summary.1,
            "total_cost_usd": summary.2.to_string(),
            "total_savings_usd": summary.3.to_string(),
            "router_savings_usd": summary.4.to_string(),
            "fallback_savings_usd": summary.5.to_string(),
            "cache_savings_usd": summary.6.to_string(),
            "truncated": summary.7,
            "cache_affinity_requests": summary.8,
        },
        "waste": waste,
        "top": top,
        "quality": {
            "error_rate": rate(quality.1, quality.0),
            "truncated_rate": rate(quality.2, quality.0),
            "guard_fallback_rate": rate(quality.3, quality.0),
            "errors_by_class": errors.into_iter().collect::<std::collections::BTreeMap<_, _>>(),
        },
    }))
}

fn decimal_from(value: Option<&str>) -> Decimal {
    value
        .and_then(|text| text.parse::<Decimal>().ok())
        .unwrap_or(Decimal::ZERO)
}

#[derive(Deserialize)]
pub struct PublicWindow {
    #[serde(default)]
    days: Option<i64>,
}

/// GET /v1/public/audit/:token — the shared, read-only report.
pub async fn public_report(
    State(state): State<std::sync::Arc<AppState>>,
    Path(token): Path<String>,
    Query(window): Query<PublicWindow>,
) -> Response {
    let store = PostgresAuditShareStore::new(state.db.clone());
    let tenant = match store.resolve(&crate::sha256_hex(&token)).await {
        Ok(Some(tenant)) => tenant,
        Ok(None) => {
            return crate::json_error(
                StatusCode::NOT_FOUND,
                "invalid_share",
                "this audit link is invalid or has been revoked",
            )
        }
        Err(e) => {
            tracing::error!("audit share lookup failed: {e}");
            return crate::json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "share_store_error",
                "could not resolve this audit link",
            )
        }
    };
    let days = match window.days {
        Some(days) if days == 7 || days == 30 || days == 90 => days,
        _ => 30,
    };
    match report(&state.db, tenant, days).await {
        Ok(report) => (StatusCode::OK, Json(report)).into_response(),
        Err(e) => {
            tracing::error!("audit report failed: {e}");
            crate::json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "report_error",
                "could not build the audit report",
            )
        }
    }
}

#[cfg(test)]
mod tests;
