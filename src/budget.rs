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
    /// ITEM-081: the tenant's own downshift rules (empty = built-in only).
    pub tier_pairs: Vec<crate::router::TierPair>,
    /// ITEM-088: store raw prompts/completions at all.
    pub capture_payloads: bool,
    /// ITEM-088: how long raw payloads live (0 = keep forever).
    pub payload_retention_days: i64,
}

/// ITEM-046: the tenant's routing/budget controls, as submitted from the UI.
#[derive(Clone, Debug)]
pub struct ConfigUpdate {
    pub budget_limit_cents: i64,
    pub cheap_fallback_model: Option<String>,
    pub guard_enabled: bool,
    pub router_enabled: bool,
    pub router_threshold: f64,
    pub capture_payloads: bool,
    pub payload_retention_days: i64,
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

/// The `tenant_configurations` row as both the hot path and the edge config sync
/// read it (ITEM-015/027/046/081).
pub(crate) type ConfigRow = (
    i64,
    i64,
    Option<String>,
    bool,
    bool,
    f64,
    sqlx::types::Json<Vec<crate::router::TierPair>>,
    bool,
    i64,
);

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

    /// ITEM-081: replace the tenant's own downshift pairs (empty clears them).
    /// The hot path reads them from the same row `budget_for` already loads.
    async fn set_tier_pairs(
        &self,
        _tenant_id: Uuid,
        _pairs: &[crate::router::TierPair],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Err("this budget store does not support tier pairs".into())
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
        let tier_pairs = rows
            .get(&tenant_id)
            .map(|budget| budget.tier_pairs.clone())
            .unwrap_or_default();
        rows.insert(
            tenant_id,
            TenantBudget {
                limit_cents: update.budget_limit_cents,
                spent_cents,
                cheap_fallback_model: update.cheap_fallback_model.clone(),
                guard_enabled: update.guard_enabled,
                router_enabled: update.router_enabled,
                router_threshold: update.router_threshold,
                tier_pairs,
                capture_payloads: update.capture_payloads,
                payload_retention_days: update.payload_retention_days,
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

    async fn set_tier_pairs(
        &self,
        tenant_id: Uuid,
        pairs: &[crate::router::TierPair],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut rows = self.rows();
        let mut budget = rows.remove(&tenant_id).unwrap_or(TenantBudget {
            limit_cents: 0,
            spent_cents: 0,
            cheap_fallback_model: None,
            guard_enabled: true,
            router_enabled: true,
            router_threshold: crate::router::DOWNGRADE_THRESHOLD,
            tier_pairs: Vec::new(),
            capture_payloads: true,
            payload_retention_days: 30,
        });
        budget.tier_pairs = pairs.to_vec();
        rows.insert(tenant_id, budget);
        Ok(())
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
        let row: Option<ConfigRow> = sqlx::query_as(
            "SELECT budget_limit_cents, current_spend_cents, cheap_fallback_model,
                    COALESCE(guard_enabled, TRUE), COALESCE(router_enabled, TRUE),
                    COALESCE(router_threshold, 0.35), tier_pairs,
                    COALESCE(payload_capture, TRUE),
                    COALESCE(payload_retention_days, 30)
             FROM tenant_configurations WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(limit_cents, spent_cents, cheap_fallback_model, guard_enabled, router_enabled, router_threshold, tier_pairs, capture_payloads, payload_retention_days)| {
                TenantBudget {
                    limit_cents,
                    spent_cents,
                    cheap_fallback_model,
                    guard_enabled,
                    router_enabled,
                    router_threshold,
                    tier_pairs: tier_pairs.0,
                    capture_payloads,
                    payload_retention_days,
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

    async fn set_tier_pairs(
        &self,
        tenant_id: Uuid,
        pairs: &[crate::router::TierPair],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query(
            "INSERT INTO tenant_configurations (tenant_id, tier_pairs) VALUES ($1, $2)
             ON CONFLICT (tenant_id) DO UPDATE SET tier_pairs = $2",
        )
        .bind(tenant_id)
        .bind(sqlx::types::Json(pairs))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_config(
        &self,
        tenant_id: Uuid,
        update: &ConfigUpdate,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query(
            "INSERT INTO tenant_configurations
                (tenant_id, budget_limit_cents, cheap_fallback_model,
                 guard_enabled, router_enabled, router_threshold,
                 payload_capture, payload_retention_days)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (tenant_id) DO UPDATE SET
                budget_limit_cents = $2, cheap_fallback_model = $3,
                guard_enabled = $4, router_enabled = $5, router_threshold = $6,
                payload_capture = $7, payload_retention_days = $8",
        )
        .bind(tenant_id)
        .bind(update.budget_limit_cents)
        .bind(update.cheap_fallback_model.as_deref())
        .bind(update.guard_enabled)
        .bind(update.router_enabled)
        .bind(update.router_threshold)
        .bind(update.capture_payloads)
        .bind(update.payload_retention_days)
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

/// A model override chosen on the hot path, with its reason (and, for the
/// heuristic router, which tier-pair table chose the target).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelOverride {
    pub model: String,
    pub reason: &'static str,
    pub pair_source: Option<crate::router::PairSource>,
}

/// The tenant's budget row, or the typed 500 response when the lookup fails.
async fn load_budget(
    state: &Arc<AppState>,
    tenant_id: Uuid,
) -> Result<Option<TenantBudget>, Response> {
    // ITEM-092: the controls come from the hot-path cache; spend is refreshed
    // on its own shorter TTL.
    state.hotpath.budget(state, tenant_id).await
}

/// ITEM-081 AC3: a tenant pair is only usable while its cheap model is still
/// routable. Write-time validation cannot see a model disabled later, so the hot
/// path keeps the client's model instead of asking for a model nothing serves.
async fn pair_is_routable(state: &Arc<AppState>, tenant_id: Uuid, cheap: &str) -> bool {
    let Ok(providers) = state
        .provider_store
        .providers_for_model(tenant_id, cheap)
        .await
    else {
        return true; // a store hiccup must not silently switch routing off
    };
    if !providers.is_empty() {
        return true;
    }
    // No provider advertises it: fine only when the tenant has no providers at
    // all, where the shared platform key accepts any model.
    state
        .provider_store
        .resolve_for(tenant_id, None)
        .await
        .ok()
        .flatten()
        .is_none()
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
            pair_source: Some(crate::router::PairSource::Global),
        }));
    };
    match enforce_budget(&budget) {
        BudgetVerdict::Allow => {
            if !budget.router_enabled {
                return Ok(None);
            }
            let threshold = policy_threshold.unwrap_or(budget.router_threshold);
            let Some(chosen) =
                crate::router::maybe_downgrade_with_pairs(payload, threshold, &budget.tier_pairs)
            else {
                return Ok(None);
            };
            if chosen.source == crate::router::PairSource::Tenant
                && !pair_is_routable(state, tenant_id, &chosen.model).await
            {
                return Ok(None);
            }
            Ok(Some(ModelOverride {
                model: chosen.model,
                reason: ROUTER_REASON,
                pair_source: Some(chosen.source),
            }))
        }
        BudgetVerdict::Reject => Err(reject_response()),
        BudgetVerdict::DowngradeTo(model) => Ok(Some(ModelOverride {
            model,
            reason: BUDGET_REASON,
            pair_source: None,
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
