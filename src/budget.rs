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

/// Apply the breaker AND the heuristic router on the hot path, before
/// forwarding. Returns the model the request should use (`None` = keep the
/// client's model) or a rejection response when the hard cap is hit.
pub async fn check_budget(
    state: &Arc<AppState>,
    tenant_id: Uuid,
    payload: &Value,
) -> Result<Option<ModelOverride>, Response> {
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
    // Unconfigured tenant (no row): the router still runs with its defaults
    // (ITEM-015 behaviour unchanged); an explicit config can turn it off or
    // move the threshold (ITEM-046).
    let Some(budget) = budget else {
        return Ok(crate::router::maybe_downgrade(payload).map(|model| ModelOverride {
            model,
            reason: ROUTER_REASON,
        }));
    };
    match enforce_budget(&budget) {
        BudgetVerdict::Allow => Ok(if budget.router_enabled {
            crate::router::maybe_downgrade_with(payload, budget.router_threshold).map(|model| {
                ModelOverride {
                    model,
                    reason: ROUTER_REASON,
                }
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
            router_enabled: true,
            router_threshold: 0.35,
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
                router_enabled: true,
                router_threshold: 0.35,
            },
        );
        let store = InMemoryBudgetStore::new(budgets);
        // Unconfigured tenant (no row) -> guarded (default true).
        assert!(store.guard_enabled_for(uuid::Uuid::new_v4()).await.unwrap());
        // Configured opt-out -> false.
        assert!(!store.guard_enabled_for(tenant).await.unwrap());
    }

    #[tokio::test]
    async fn update_config_upserts_and_preserves_spend() {
        // ITEM-046: the controls write through the same seam the breaker reads.
        let tenant = uuid::Uuid::new_v4();
        let mut budgets = std::collections::HashMap::new();
        budgets.insert(tenant, budget(1000, 250, Some("gpt-4o-mini")));
        let store = InMemoryBudgetStore::new(budgets);
        store
            .update_config(
                tenant,
                &ConfigUpdate {
                    budget_limit_cents: 5000,
                    cheap_fallback_model: Some("deepseek-chat".to_string()),
                    guard_enabled: false,
                    router_enabled: false,
                    router_threshold: 0.8,
                },
            )
            .await
            .unwrap();
        let after = store.budget_for(tenant).await.unwrap().unwrap();
        assert_eq!(after.limit_cents, 5000);
        assert_eq!(after.spent_cents, 250, "telemetry owns spend; the update never resets it");
        assert_eq!(after.cheap_fallback_model.as_deref(), Some("deepseek-chat"));
        assert!(!after.guard_enabled);
        assert!(!after.router_enabled);
        assert_eq!(after.router_threshold, 0.8);
        // An unconfigured tenant gets a row on first update (defaults preserved).
        let fresh = uuid::Uuid::new_v4();
        store
            .update_config(
                fresh,
                &ConfigUpdate {
                    budget_limit_cents: 0,
                    cheap_fallback_model: None,
                    guard_enabled: true,
                    router_enabled: true,
                    router_threshold: 0.35,
                },
            )
            .await
            .unwrap();
        assert!(store.budget_for(fresh).await.unwrap().is_some());
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
