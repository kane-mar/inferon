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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TenantBudget {
    pub limit_cents: i64,
    pub spent_cents: i64,
    pub cheap_fallback_model: Option<String>,
    pub guard_enabled: bool,
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
pub fn enforce_budget(budget: &TenantBudget) -> BudgetVerdict {
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
}

#[derive(Default)]
pub struct InMemoryBudgetStore {
    budgets: HashMap<Uuid, TenantBudget>,
}

impl InMemoryBudgetStore {
    pub fn new(budgets: HashMap<Uuid, TenantBudget>) -> Self {
        Self { budgets }
    }
}

#[async_trait::async_trait]
impl BudgetStore for InMemoryBudgetStore {
    async fn budget_for(
        &self,
        tenant_id: Uuid,
    ) -> Result<Option<TenantBudget>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.budgets.get(&tenant_id).cloned())
    }

    async fn guard_enabled_for(
        &self,
        tenant_id: Uuid,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.budgets.get(&tenant_id).map_or(true, |b| b.guard_enabled))
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
        let row: Option<(i64, i64, Option<String>, bool)> = sqlx::query_as(
            "SELECT budget_limit_cents, current_spend_cents, cheap_fallback_model,
                    COALESCE(guard_enabled, TRUE)
             FROM tenant_configurations WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(limit_cents, spent_cents, cheap_fallback_model, guard_enabled)| {
            TenantBudget {
                limit_cents,
                spent_cents,
                cheap_fallback_model,
                guard_enabled,
            }
        }))
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
}

/// HTTP response for a budget rejection (AC: 429 with an OpenAI-style body).
fn reject_response() -> Response {
    crate::json_error(
        StatusCode::TOO_MANY_REQUESTS,
        "budget_exceeded",
        "monthly budget exhausted",
    )
}

/// Apply the breaker on the hot path, before forwarding. Returns the model the
/// request should use (`None` = keep the client's model) or a rejection
/// response when the hard cap is hit.
pub async fn check_budget(
    state: &Arc<AppState>,
    tenant_id: Uuid,
) -> Result<Option<String>, Response> {
    let budget = match state.budget_store.budget_for(tenant_id).await {
        Ok(budget) => budget,
        Err(e) => {
            tracing::error!("budget lookup failed: {e}");
            return Err(crate::json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "budget_store_error",
                "budget lookup failed",
            ));
        }
    };
    let Some(budget) = budget else {
        return Ok(None);
    };
    match enforce_budget(&budget) {
        BudgetVerdict::Allow => Ok(None),
        BudgetVerdict::Reject => Err(reject_response()),
        BudgetVerdict::DowngradeTo(model) => Ok(Some(model)),
    }
}

/// Override the request model with `model` if it differs, returning the body
/// bytes to forward (payload reserialized) — shared by the router downgrade
/// and the budget fallback.
pub fn apply_model_override(payload: &Value, model: &str) -> Result<Vec<u8>, serde_json::Error> {
    let mut forwarded = payload.clone();
    forwarded["model"] = serde_json::Value::String(model.to_string());
    serde_json::to_vec(&forwarded)
}
#[cfg(test)]
mod tests {
    use super::*;

    fn budget(limit: i64, spent: i64, cheap: Option<&str>) -> TenantBudget {
        TenantBudget {
            limit_cents: limit,
            spent_cents: spent,
            cheap_fallback_model: cheap.map(str::to_string),
            guard_enabled: true,
        }
    }

    #[test]
    fn unlimited_and_unconfigured_always_allow() {
        assert_eq!(enforce_budget(&budget(0, 0, None)), BudgetVerdict::Allow);
        assert_eq!(enforce_budget(&budget(0, 99_999, Some("m"))), BudgetVerdict::Allow);
    }

    #[test]
    fn hard_cap_rejects() {
        assert_eq!(enforce_budget(&budget(1000, 1000, None)), BudgetVerdict::Reject);
        assert_eq!(enforce_budget(&budget(1000, 2500, Some("m"))), BudgetVerdict::Reject);
    }

    #[test]
    fn last_fifteen_percent_downgrades_when_model_available() {
        // 84% spent -> Allow (integer math: 840*20 < 1000*17=17000 -> 16800 < 17000)
        assert_eq!(enforce_budget(&budget(1000, 840, Some("gpt-4o-mini"))), BudgetVerdict::Allow);
        // 85% spent -> DowngradeTo
        assert_eq!(
            enforce_budget(&budget(1000, 850, Some("gpt-4o-mini"))),
            BudgetVerdict::DowngradeTo("gpt-4o-mini".to_string())
        );
        // in-zone but no cheap model configured -> Allow (no fallback possible)
        assert_eq!(enforce_budget(&budget(1000, 900, None)), BudgetVerdict::Allow);
    }

    #[tokio::test]
    async fn guard_is_on_by_default_and_opt_out_is_honored() {
        let tenant = uuid::Uuid::new_v4();
        let mut budgets = std::collections::HashMap::new();
        budgets.insert(
            tenant,
            TenantBudget {
                limit_cents: 0,
                spent_cents: 0,
                cheap_fallback_model: None,
                guard_enabled: false,
            },
        );
        let store = InMemoryBudgetStore::new(budgets);
        // Unconfigured tenant (no row) -> guarded (default true).
        assert!(store.guard_enabled_for(uuid::Uuid::new_v4()).await.unwrap());
        // Configured opt-out -> false.
        assert!(!store.guard_enabled_for(tenant).await.unwrap());
    }

    #[test]
    fn model_override_reserializes_with_new_model() {
        let payload = serde_json::json!({
            "model": "gpt-4o",
            "stream": false,
            "messages": [{"role": "user", "content": "hi"}],
        });
        let bytes = apply_model_override(&payload, "gpt-4o-mini").unwrap();
        let out: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(out["model"], "gpt-4o-mini");
        assert_eq!(out["messages"], payload["messages"], "only model changes");
    }
}
