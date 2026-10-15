//! Postgres budget circuit breaker (Slice 2, ITEM-016).
//!
//! Per project decision: the spend counter is Postgres (`tenant_configurations`,
//! integer cents), read on the hot path before forwarding and incremented off
//! the hot path at telemetry flush time from the actual cost. Redis fast-path +
//! async reconciliation is the documented scale-out path, not built here.
//!
//! Limits: `budget_limit_cents` (0 = unlimited); cheap-fallback zone is the
//! last 15% of the budget (>= 85% spent); hard cap rejects with 429.

use crate::AppState;
use axum::{http::StatusCode, response::Response};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use uuid::Uuid;

/// Config for one tenant (0 limit = unlimited; None model = no fallback).
/// `guard_enabled` (ITEM-027 AC1) lives on the same `tenant_configurations`
/// row the budget reads — the schema guard is on by default and a tenant can
/// opt out here.
#[derive(Clone, Debug, PartialEq)]
pub struct TenantBudget {
    pub limit_cents: i64,
    pub spent_cents: i64,
    pub cheap_fallback_model: Option<String>,
    pub guard_enabled: bool,
    /// ITEM-046: the heuristic router's controls.
    pub router_enabled: bool,
    pub router_threshold: f64,
}

/// ITEM-046: the tenant's routing/budget controls, as submitted from the UI.
#[derive(Clone, Debug)]
pub struct ConfigUpdate {
    pub budget_limit_cents: i64,
    pub cheap_fallback_model: Option<String>,
    pub guard_enabled: bool,
    pub router_enabled: bool,
    pub router_threshold: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetVerdict {
    Allow,
    Reject,
    DowngradeTo(String),
}

/// Pure enforcement: hard cap -> Reject; inside the last 15% with a cheap
/// model configured -> DowngradeTo; otherwise Allow. Integer math (cents),
/// never float: 85% zone test is `spent * 20 >= limit * 17`.
pub(crate) fn enforce_budget(budget: &TenantBudget) -> BudgetVerdict {
    if budget.limit_cents <= 0 {
        return BudgetVerdict::Allow; // unconfigured / unlimited
    }
    if budget.spent_cents >= budget.limit_cents {
        return BudgetVerdict::Reject;
    }
    if budget.spent_cents * 20 >= budget.limit_cents * 17 {
        if let Some(model) = &budget.cheap_fallback_model {
            return BudgetVerdict::DowngradeTo(model.clone());
        }
    }
    BudgetVerdict::Allow
}

/// Lookup seam: budget config per tenant. `None` = no config = unlimited.
#[async_trait::async_trait]
pub trait BudgetStore: Send + Sync {
    async fn budget_for(
        &self,
        tenant_id: Uuid,
    ) -> Result<Option<TenantBudget>, Box<dyn std::error::Error + Send + Sync>>;

    /// ITEM-027 AC1: whether the schema guard is active for this tenant.
    /// Defaults to true — the guard is on unless the tenant's configuration
    /// row opts out (`guard_enabled = false`); a config-read failure also
    /// stays true (fail-open: never weaken the safety net on a DB hiccup).
    async fn guard_enabled_for(
        &self,
        _tenant_id: Uuid,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        Ok(true)
    }

    /// ITEM-046: upsert the tenant's routing/budget controls. `current_spend_cents`
    /// is owned by telemetry and is never touched here.
    async fn update_config(
        &self,
        _tenant_id: Uuid,
        _update: &ConfigUpdate,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Err("this budget store does not support configuration".into())
    }
}

#[derive(Default)]
pub struct InMemoryBudgetStore {
    budgets: std::sync::Mutex<HashMap<Uuid, TenantBudget>>,
}

impl InMemoryBudgetStore {
    pub fn new(budgets: HashMap<Uuid, TenantBudget>) -> Self {
        Self {
            budgets: std::sync::Mutex::new(budgets),
        }
    }

    fn rows(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, TenantBudget>> {
        self.budgets.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[async_trait::async_trait]
impl BudgetStore for InMemoryBudgetStore {
    async fn budget_for(
        &self,
        tenant_id: Uuid,
    ) -> Result<Option<TenantBudget>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.rows().get(&tenant_id).cloned())
    }

    async fn update_config(
        &self,
        tenant_id: Uuid,
        update: &ConfigUpdate,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut rows = self.rows();
        let spent_cents = rows.get(&tenant_id).map(|b| b.spent_cents).unwrap_or(0);
        rows.insert(
            tenant_id,
            TenantBudget {
                limit_cents: update.budget_limit_cents,
                spent_cents,
                cheap_fallback_model: update.cheap_fallback_model.clone(),
                guard_enabled: update.guard_enabled,
                router_enabled: update.router_enabled,
                router_threshold: update.router_threshold,
            },
        );
        Ok(())
    }

    async fn guard_enabled_for(
        &self,
        tenant_id: Uuid,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.rows().get(&tenant_id).map_or(true, |b| b.guard_enabled))
    }
}

pub struct PostgresBudgetStore {
    pool: sqlx::PgPool,
}

impl PostgresBudgetStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl BudgetStore for PostgresBudgetStore {
    async fn budget_for(
        &self,
        tenant_id: Uuid,
    ) -> Result<Option<TenantBudget>, Box<dyn std::error::Error + Send + Sync>> {
        let row: Option<(i64, i64, Option<String>, bool, bool, f64)> = sqlx::query_as(
            "SELECT budget_limit_cents, current_spend_cents, cheap_fallback_model,
                    COALESCE(guard_enabled, TRUE), COALESCE(router_enabled, TRUE),
                    COALESCE(router_threshold, 0.35)
             FROM tenant_configurations WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(limit_cents, spent_cents, cheap_fallback_model, guard_enabled, router_enabled, router_threshold)| {
                TenantBudget {
                    limit_cents,
                    spent_cents,
                    cheap_fallback_model,
                    guard_enabled,
                    router_enabled,
                    router_threshold,
                }
            },
        ))
    }

    async fn guard_enabled_for(
        &self,
        tenant_id: Uuid,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let enabled: Option<bool> = sqlx::query_scalar(
            "SELECT COALESCE(guard_enabled, TRUE) FROM tenant_configurations
             WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(enabled.unwrap_or(true))
    }

    async fn update_config(
        &self,
        tenant_id: Uuid,
        update: &ConfigUpdate,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query(
            "INSERT INTO tenant_configurations
                (tenant_id, budget_limit_cents, cheap_fallback_model,
                 guard_enabled, router_enabled, router_threshold)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (tenant_id) DO UPDATE SET
                budget_limit_cents = $2, cheap_fallback_model = $3,
                guard_enabled = $4, router_enabled = $5, router_threshold = $6",
        )
        .bind(tenant_id)
        .bind(update.budget_limit_cents)
        .bind(update.cheap_fallback_model.as_deref())
        .bind(update.guard_enabled)
        .bind(update.router_enabled)
        .bind(update.router_threshold)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

/// HTTP response for a budget rejection (AC: 429 with an OpenAI-style body).
fn reject_response() -> Response {
    crate::json_error(
        StatusCode::TOO_MANY_REQUESTS,
        "budget_exceeded",
        "monthly budget exhausted",
    )
}

/// ITEM-050: enforce only the hard monthly cap for a non-OpenAI surface. The
/// heuristic router and the cheap-fallback override are OpenAI-shaped, so an
/// Anthropic request is served as asked or rejected at the cap — never rewritten.
pub async fn check_cap(state: &Arc<AppState>, tenant_id: Uuid) -> Result<(), Response> {
    let budget = load_budget(state, tenant_id).await?;
    match budget.map(|budget| enforce_budget(&budget)) {
        Some(BudgetVerdict::Reject) => Err(reject_response()),
        _ => Ok(()),
    }
}

/// Why the request model was overridden (ITEM-054: recorded on the trace).
pub const ROUTER_REASON: &str = "heuristic_router";
pub const BUDGET_REASON: &str = "budget";

/// A model override chosen on the hot path, with its reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelOverride {
    pub model: String,
    pub reason: &'static str,
}

/// The tenant's budget row, or the typed 500 response when the lookup fails.
async fn load_budget(
    state: &Arc<AppState>,
    tenant_id: Uuid,
) -> Result<Option<TenantBudget>, Response> {
    state.budget_store.budget_for(tenant_id).await.map_err(|e| {
        tracing::error!("budget lookup failed: {e}");
        crate::json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "budget_store_error",
            "budget lookup failed",
        )
    })
}

/// Apply the breaker AND the heuristic router on the hot path, before
/// forwarding. Returns the model the request should use (`None` = keep the
/// client's model) or a rejection response when the hard cap is hit.
pub async fn check_budget(
    state: &Arc<AppState>,
    tenant_id: Uuid,
    payload: &Value,
    policy_threshold: Option<f64>,
) -> Result<Option<ModelOverride>, Response> {
    let budget = load_budget(state, tenant_id).await?;
    // Unconfigured tenant (no row): the router still runs. The adaptive policy
    // (ITEM-064) may supply the threshold, else the router's default.
    let Some(budget) = budget else {
        let chosen = match policy_threshold {
            Some(threshold) => crate::router::maybe_downgrade_with(payload, threshold),
            None => crate::router::maybe_downgrade(payload),
        };
        return Ok(chosen.map(|model| ModelOverride {
            model,
            reason: ROUTER_REASON,
        }));
    };
    match enforce_budget(&budget) {
        BudgetVerdict::Allow => Ok(if budget.router_enabled {
            let threshold = policy_threshold.unwrap_or(budget.router_threshold);
            crate::router::maybe_downgrade_with(payload, threshold).map(|model| ModelOverride {
                model,
                reason: ROUTER_REASON,
            })
        } else {
            None
        }),
        BudgetVerdict::Reject => Err(reject_response()),
        BudgetVerdict::DowngradeTo(model) => Ok(Some(ModelOverride {
            model,
            reason: BUDGET_REASON,
        })),
    }
}

/// Override the request model with `model` if it differs, returning the body
/// bytes to forward (payload reserialized) — shared by the router downgrade
/// and the budget fallback.
/// The upstream body for `model` (ITEM-067): the client's payload with `model`
/// set, and — when the target cannot honour a `json_schema` contract —
/// `response_format` downgraded to `json_object`. The guard still holds the real
/// schema from the original payload and validates the response against it.
pub fn forward_body(
    payload: &Value,
    model: &str,
    downgrade_schema: bool,
) -> Result<Vec<u8>, serde_json::Error> {
    let mut forwarded = payload.clone();
    forwarded["model"] = serde_json::Value::String(model.to_string());
    if downgrade_schema {
        forwarded["response_format"] = serde_json::json!({"type": "json_object"});
    }
    serde_json::to_vec(&forwarded)
}
#[cfg(test)]
mod tests;
