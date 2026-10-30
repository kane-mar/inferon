//! The hot path's read cache (ITEM-092).
//!
//! A request must not pay a database round trip per decision. Everything the
//! request path reads about a tenant — its budget controls, adaptive policy,
//! fallback chain, the resolved route per model, and the warm-cache target per
//! prompt — is cached per tenant for a short, bounded time:
//!
//! | read | TTL | why it is safe |
//! |---|---|---|
//! | config (budget, policy, fallbacks) | 5 s | writes invalidate immediately |
//! | spend (`current_spend_cents`) | 1 s | already ≤ 500 ms stale (telemetry flush) |
//! | route (provider + disabled flag) | 5 s | provider/model writes invalidate |
//! | warm target (affinity) | 30 s | a miss only loses a cache win |
//!
//! **Authentication is not cached**: resolving the key stays one fresh indexed
//! lookup, so revoking a key still stops it on the next request (ITEM-034).
//! That is the one storage round trip a warm request pays.
//!
//! Invalidation is explicit (`invalidate`) on every dashboard write, with the TTL
//! as the backstop for changes made outside the process (SQL, workers).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::affinity::WarmTarget;
use crate::budget::TenantBudget;
use crate::policy::Policy;
use crate::proxy::{resolve_route, Route, Surface};
use crate::tenant::Tenant;
use crate::AppState;

/// How long each cached read stays valid.
#[derive(Clone, Copy, Debug)]
pub struct Ttls {
    pub config: Duration,
    pub spend: Duration,
    pub warm: Duration,
    /// Key-hash -> tenant. Short, and invalidated on every key write: a revoked
    /// key stops working on the next request in this process. A second proxy
    /// instance would lag by at most this TTL.
    pub auth: Duration,
}

impl Default for Ttls {
    fn default() -> Self {
        Self {
            config: Duration::from_secs(5),
            spend: Duration::from_secs(1),
            warm: Duration::from_secs(30),
            auth: Duration::from_secs(1),
        }
    }
}

/// A resolved route plus the tenant's disable flag for that model.
#[derive(Clone)]
pub struct Decision {
    pub(crate) route: Route,
    pub(crate) disabled: bool,
}

struct TenantEntry {
    refreshed: Option<Instant>,
    /// The tenant's controls; `spent_cents` is held at zero here and filled from
    /// the spend cache when read (`spend` must not be 5 s stale).
    budget: Option<TenantBudget>,
    policy: Option<Policy>,
    fallbacks: Vec<String>,
    spend: Option<(i64, Instant)>,
    routes: HashMap<String, Decision>,
    warm: HashMap<String, (Option<WarmTarget>, Instant)>,
}

impl TenantEntry {
    fn new() -> Self {
        Self {
            refreshed: None,
            budget: None,
            policy: None,
            fallbacks: Vec::new(),
            spend: None,
            routes: HashMap::new(),
            warm: HashMap::new(),
        }
    }
}

pub struct HotPath {
    ttls: Ttls,
    tenants: Mutex<HashMap<Uuid, Arc<Mutex<TenantEntry>>>>,
    /// Key hash -> tenant. The one thing the request path would otherwise read
    /// from Postgres; invalidation on key writes keeps revocation immediate.
    auth: Mutex<HashMap<String, (Tenant, Instant)>>,
}

impl Default for HotPath {
    fn default() -> Self {
        Self::new()
    }
}

impl HotPath {
    pub fn new() -> Self {
        Self::with_ttls(Ttls::default())
    }

    pub fn with_ttls(ttls: Ttls) -> Self {
        Self {
            ttls,
            tenants: Mutex::new(HashMap::new()),
            auth: Mutex::new(HashMap::new()),
        }
    }

    /// Drop everything cached for a tenant. Call after any write that changes
    /// its controls, providers, models or routing.
    pub fn invalidate(&self, tenant_id: Uuid) {
        self.tenants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&tenant_id);
        self.auth
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, (tenant, _)| tenant.id != tenant_id);
    }

    /// Drop one key's cached identity. Used when only the key id is known (the
    /// operator revoke path); the tenant's other caches are unaffected.
    pub fn invalidate_key(&self, key_id: Uuid) {
        self.auth
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, (tenant, _)| tenant.api_key_id != Some(key_id));
    }

    /// Resolve a key hash to its tenant, cached for the (short) auth TTL. A miss
    /// is one indexed lookup; a hit is pure memory.
    pub async fn tenant_for_key(
        &self,
        state: &Arc<AppState>,
        key_hash: &str,
    ) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>> {
        {
            let auth = self
                .auth
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some((tenant, at)) = auth.get(key_hash) {
                if at.elapsed() < self.ttls.auth {
                    return Ok(Some(tenant.clone()));
                }
            }
        }
        let tenant = state.tenant_store.tenant_by_api_key_hash(key_hash).await?;
        if let Some(tenant) = &tenant {
            self.auth
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(key_hash.to_string(), (tenant.clone(), Instant::now()));
        }
        Ok(tenant)
    }

    fn entry(&self, tenant_id: Uuid) -> Arc<Mutex<TenantEntry>> {
        let mut tenants = self
            .tenants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        tenants
            .entry(tenant_id)
            .or_insert_with(|| Arc::new(Mutex::new(TenantEntry::new())))
            .clone()
    }

    /// Refresh the control-plane snapshot when it is older than the config TTL.
    /// A budget-store error is a request error (the cap cannot be evaluated);
    /// policy/fallback errors degrade quietly, as they always have.
    async fn refresh(
        &self,
        state: &Arc<AppState>,
        tenant_id: Uuid,
    ) -> Result<Arc<Mutex<TenantEntry>>, axum::response::Response> {
        let entry = self.entry(tenant_id);
        let stale = {
            let e = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            e.refreshed.is_none_or(|at| at.elapsed() >= self.ttls.config)
        };
        if !stale {
            return Ok(entry);
        }
        let budget = match state.budget_store.budget_for(tenant_id).await {
            Ok(budget) => budget.map(|mut budget| {
                budget.spent_cents = 0; // spend is read fresh, never from here
                budget
            }),
            Err(_) => {
                return Err(crate::json_error(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "budget_store_error",
                    "budget lookup failed",
                ))
            }
        };
        let policy = state
            .policy_store
            .active(tenant_id)
            .await
            .ok()
            .flatten()
            .filter(|policy| policy.enabled);
        let fallbacks = state.routing_store.fallbacks(tenant_id).await.unwrap_or_else(|e| {
            tracing::error!("routing policy load failed: {e}");
            Vec::new()
        });
        {
            let mut e = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // Another task may have refreshed while we were reading.
            if e.refreshed.is_none_or(|at| at.elapsed() >= self.ttls.config) {
                e.refreshed = Some(Instant::now());
                e.budget = budget;
                e.policy = policy;
                e.fallbacks = fallbacks;
                e.routes.clear();
            }
        }
        Ok(entry)
    }

    /// The tenant's controls, with a spend figure no older than the spend TTL.
    pub async fn budget(
        &self,
        state: &Arc<AppState>,
        tenant_id: Uuid,
    ) -> Result<Option<TenantBudget>, axum::response::Response> {
        let entry = self.refresh(state, tenant_id).await?;
        let spend = self.spend(state, tenant_id, &entry).await?;
        let e = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(e.budget.clone().map(|budget| TenantBudget {
            spent_cents: spend,
            ..budget
        }))
    }

    async fn spend(
        &self,
        state: &Arc<AppState>,
        tenant_id: Uuid,
        entry: &Arc<Mutex<TenantEntry>>,
    ) -> Result<i64, axum::response::Response> {
        let cached = {
            let e = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            e.spend
                .filter(|(_, at)| at.elapsed() < self.ttls.spend)
                .map(|(value, _)| value)
        };
        if let Some(value) = cached {
            return Ok(value);
        }
        match state.budget_store.budget_for(tenant_id).await {
            Ok(budget) => {
                let value = budget.map(|budget| budget.spent_cents).unwrap_or(0);
                let mut e = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                e.spend = Some((value, Instant::now()));
                Ok(value)
            }
            Err(e) => {
                tracing::error!("budget lookup failed: {e}");
                Err(crate::json_error(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "budget_store_error",
                    "budget lookup failed",
                ))
            }
        }
    }

    /// The guard switch, fail-open (a store error never removes the safety net).
    pub async fn guard_enabled(&self, state: &Arc<AppState>, tenant_id: Uuid) -> bool {
        match self.budget(state, tenant_id).await {
            Ok(Some(budget)) => budget.guard_enabled,
            _ => true,
        }
    }

    pub async fn policy(&self, state: &Arc<AppState>, tenant_id: Uuid) -> Option<Policy> {
        match self.refresh(state, tenant_id).await {
            Ok(entry) => entry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .policy
                .clone(),
            Err(_) => None,
        }
    }

    pub async fn fallbacks(&self, state: &Arc<AppState>, tenant_id: Uuid) -> Vec<String> {
        match self.refresh(state, tenant_id).await {
            Ok(entry) => entry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .fallbacks
                .clone(),
            Err(_) => Vec::new(),
        }
    }

    /// Resolve a model to a provider, caching the decision (including whether the
    /// tenant disabled the model). Errors are never cached: they are rare and
    /// usually the caller's to report.
    pub(crate) async fn route(
        &self,
        state: &Arc<AppState>,
        tenant: &Tenant,
        model: &str,
        surface: Surface,
        weights: Option<&std::collections::BTreeMap<String, f64>>,
    ) -> Result<Decision, axum::response::Response> {
        let key = format!("{surface:?}:{model}");
        let entry = self.entry(tenant.id);
        if let Some(hit) = entry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .routes
            .get(&key)
        {
            return Ok(hit.clone());
        }
        let route = resolve_route(state, tenant, Some(model), surface, weights).await?;
        let disabled = state
            .model_store
            .is_disabled(route.provider_id, model)
            .await
            .unwrap_or(false);
        let decision = Decision { route, disabled };
        entry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .routes
            .insert(key, decision.clone());
        Ok(decision)
    }

    /// The warm-cache target for a prompt prefix (ITEM-063), cached for the warm
    /// TTL. A miss costs a cache win, never correctness.
    pub async fn warm_target(
        &self,
        state: &Arc<AppState>,
        tenant_id: Uuid,
        hash: &str,
    ) -> Option<WarmTarget> {
        let entry = self.entry(tenant_id);
        let cached = {
            let e = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            e.warm
                .get(hash)
                .filter(|(_, at)| at.elapsed() < self.ttls.warm)
                .map(|(target, _)| target.clone())
        };
        if let Some(cached) = cached {
            return cached;
        }
        let target = state
            .affinity_store
            .warm_target(tenant_id, hash)
            .await
            .unwrap_or(None);
        entry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .warm
            .insert(hash.to_string(), (target.clone(), Instant::now()));
        target
    }
}
