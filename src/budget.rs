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
    /// ITEM-090: run the pre-dispatch lint pass (kill switch).
    pub lint_enabled: bool,
    /// ITEM-090: which rules run (all of them when NULL, i.e. unset).
    pub lint_rules: Vec<String>,
    /// ITEM-094: translate across surfaces when the provider speaks the other one.
    pub translate_surfaces: bool,
    /// ITEM-095: the tenant's default velocity limits (0 = unlimited).
    pub rpm_limit: i64,
    pub tpm_limit: i64,
    /// ITEM-096: scan payloads for PII shapes (counts only).
    pub pii_detection: bool,
    /// ITEM-096: replace matches in the stored payload.
    pub pii_redact_storage: bool,
    /// ITEM-096: replace matches in the forwarded request.
    pub pii_redact_wire: bool,
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
    /// ITEM-090: the pre-flight lint kill switch.
    pub lint_enabled: bool,
    /// ITEM-090: the rules to run (names from `preflight::ALL_RULES`).
    pub lint_rules: Vec<String>,
    /// ITEM-094: cross-surface translation.
    pub translate_surfaces: bool,
    /// ITEM-095: the tenant's default velocity limits (0 = unlimited).
    pub rpm_limit: i64,
    pub tpm_limit: i64,
    /// ITEM-096: detection and the two redaction modes.
    pub pii_detection: bool,
    pub pii_redact_storage: bool,
    pub pii_redact_wire: bool,
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

/// The `tenant_configurations` row, decoded **by column name**.
///
/// This used to be a 16-element tuple: every new tenant switch added a position,
/// and a single miscount shifted every following field into the wrong type. It
/// is now a struct with the column aliases spelled out, and `CONFIG_SELECT` is
/// asserted against it (see `tests::the_config_select_covers_every_field`).
pub(crate) struct ConfigRow {
    pub limit_cents: i64,
    pub spent_cents: i64,
    pub cheap_fallback_model: Option<String>,
    pub guard_enabled: bool,
    pub router_enabled: bool,
    pub router_threshold: f64,
    pub tier_pairs: sqlx::types::Json<Vec<crate::router::TierPair>>,
    pub capture_payloads: bool,
    pub payload_retention_days: i64,
    pub lint_enabled: bool,
    pub lint_rules: Option<Vec<String>>,
    pub translate_surfaces: bool,
    pub rpm_limit: i64,
    pub tpm_limit: i64,
    pub pii_detection: bool,
    pub pii_redact_storage: bool,
    pub pii_redact_wire: bool,
}

/// Every column the row decoder expects, with its alias. One place, so the
/// query and the struct cannot drift apart.
pub(crate) const CONFIG_SELECT: &str = "SELECT budget_limit_cents AS limit_cents,
                current_spend_cents AS spent_cents,
                cheap_fallback_model AS cheap_fallback_model,
                COALESCE(guard_enabled, TRUE) AS guard_enabled,
                COALESCE(router_enabled, TRUE) AS router_enabled,
                COALESCE(router_threshold, 0.35) AS router_threshold,
                tier_pairs AS tier_pairs,
                COALESCE(payload_capture, TRUE) AS capture_payloads,
                COALESCE(payload_retention_days, 30) AS payload_retention_days,
                COALESCE(lint_enabled, TRUE) AS lint_enabled,
                lint_rules AS lint_rules,
                COALESCE(translate_surfaces, TRUE) AS translate_surfaces,
                COALESCE(rpm_limit, 0) AS rpm_limit,
                COALESCE(tpm_limit, 0) AS tpm_limit,
                COALESCE(pii_detection, TRUE) AS pii_detection,
                COALESCE(pii_redact_storage, FALSE) AS pii_redact_storage,
                COALESCE(pii_redact_wire, FALSE) AS pii_redact_wire
         FROM tenant_configurations
         WHERE tenant_id = $1";

// Decoded by column *name*, not position: the whole point of the struct.
impl sqlx::FromRow<'_, sqlx::postgres::PgRow> for ConfigRow {
    fn from_row(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(Self {
            limit_cents: row.try_get("limit_cents")?,
            spent_cents: row.try_get("spent_cents")?,
            cheap_fallback_model: row.try_get("cheap_fallback_model")?,
            guard_enabled: row.try_get("guard_enabled")?,
            router_enabled: row.try_get("router_enabled")?,
            router_threshold: row.try_get("router_threshold")?,
            tier_pairs: row.try_get("tier_pairs")?,
            capture_payloads: row.try_get("capture_payloads")?,
            payload_retention_days: row.try_get("payload_retention_days")?,
            lint_enabled: row.try_get("lint_enabled")?,
            lint_rules: row.try_get("lint_rules")?,
            translate_surfaces: row.try_get("translate_surfaces")?,
            rpm_limit: row.try_get("rpm_limit")?,
            tpm_limit: row.try_get("tpm_limit")?,
            pii_detection: row.try_get("pii_detection")?,
            pii_redact_storage: row.try_get("pii_redact_storage")?,
            pii_redact_wire: row.try_get("pii_redact_wire")?,
        })
    }
}

impl From<ConfigRow> for TenantBudget {
    fn from(row: ConfigRow) -> Self {
        Self {
            limit_cents: row.limit_cents,
            spent_cents: row.spent_cents,
            cheap_fallback_model: row.cheap_fallback_model,
            guard_enabled: row.guard_enabled,
            router_enabled: row.router_enabled,
            router_threshold: row.router_threshold,
            tier_pairs: row.tier_pairs.0,
            capture_payloads: row.capture_payloads,
            payload_retention_days: row.payload_retention_days,
            lint_enabled: row.lint_enabled,
            // NULL = every rule (pre-052 rows read this way).
            lint_rules: row.lint_rules.unwrap_or_else(|| crate::preflight::Rules::default().names()),
            translate_surfaces: row.translate_surfaces,
            rpm_limit: row.rpm_limit,
            tpm_limit: row.tpm_limit,
            pii_detection: row.pii_detection,
            pii_redact_storage: row.pii_redact_storage,
            pii_redact_wire: row.pii_redact_wire,
        }
    }
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
                lint_enabled: update.lint_enabled,
                lint_rules: update.lint_rules.clone(),
                translate_surfaces: update.translate_surfaces,
                rpm_limit: update.rpm_limit,
                tpm_limit: update.tpm_limit,
                pii_detection: update.pii_detection,
                pii_redact_storage: update.pii_redact_storage,
                pii_redact_wire: update.pii_redact_wire,
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
            lint_enabled: true,
            lint_rules: crate::preflight::Rules::default().names(),
            translate_surfaces: true,
            rpm_limit: 0,
            tpm_limit: 0,
            pii_detection: true,
            pii_redact_storage: false,
            pii_redact_wire: false,
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
        let row: Option<ConfigRow> = sqlx::query_as(CONFIG_SELECT)
            .bind(tenant_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(TenantBudget::from))
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
                 payload_capture, payload_retention_days, lint_enabled, lint_rules,
                 translate_surfaces, rpm_limit, tpm_limit,
                 pii_detection, pii_redact_storage, pii_redact_wire)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
             ON CONFLICT (tenant_id) DO UPDATE SET
                budget_limit_cents = $2, cheap_fallback_model = $3,
                guard_enabled = $4, router_enabled = $5, router_threshold = $6,
                payload_capture = $7, payload_retention_days = $8, lint_enabled = $9,
                lint_rules = $10, translate_surfaces = $11, rpm_limit = $12,
                tpm_limit = $13, pii_detection = $14, pii_redact_storage = $15,
                pii_redact_wire = $16",
        )
        .bind(tenant_id)
        .bind(update.budget_limit_cents)
        .bind(update.cheap_fallback_model.as_deref())
        .bind(update.guard_enabled)
        .bind(update.router_enabled)
        .bind(update.router_threshold)
        .bind(update.capture_payloads)
        .bind(update.payload_retention_days)
        .bind(update.lint_enabled)
        .bind(&update.lint_rules)
        .bind(update.translate_surfaces)
        .bind(update.rpm_limit)
        .bind(update.tpm_limit)
        .bind(update.pii_detection)
        .bind(update.pii_redact_storage)
        .bind(update.pii_redact_wire)
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
