//! The hot path's read cache (ITEM-092).
//!
//! A request must not pay a database round trip per decision. Everything the
//! request path reads about a tenant — its budget controls, adaptive policy,
//! fallback chain, the resolved route per model, and the warm-cache target per
//! prompt — is cached per tenant for a short, bounded time:
//!
//! | read | TTL | why it is safe |
//! |---|---|---|
//! | config (budget, policy, fallbacks) | 30 s | writes invalidate immediately |
//! | spend (`current_spend_cents`) | 5 s | the breaker already lags the flush |
//! | route (provider + disabled flag) | until a write / new policy version | invalidated explicitly |
//! | warm target (affinity) | 60 s | a miss only loses a cache win |
//!
//! **Authentication is not cached**: resolving the key stays one fresh indexed
//! lookup, so revoking a key still stops it on the next request (ITEM-034).
//! That is the one storage round trip a warm request pays.
//!
//! Invalidation is explicit (`invalidate`) on every dashboard write, with the TTL
//! as the backstop for changes made outside the process (SQL, workers).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
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
            // A stale value is served and refreshed off-path, and every write
            // invalidates, so these bound staleness for out-of-band changes
            // (SQL, another instance) — not for anything the dashboard does.
            config: Duration::from_secs(30),
            // The budget breaker's spend figure may lag by up to this TTL, on
            // top of the telemetry flush interval (500 ms).
            spend: Duration::from_secs(5),
            warm: Duration::from_secs(60),
            // A revoked key is dropped explicitly; a second instance would lag
            // by at most this long.
            auth: Duration::from_secs(30),
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
    /// A background config rebuild is in flight (stampede guard).
    refreshing: bool,
    /// A background spend refresh is in flight.
    revalidating_spend: bool,
    /// The policy version the cached routes were resolved under. A new version
    /// drops them, so weights and thresholds take effect.
    routes_version: Option<i64>,
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
            refreshing: false,
            revalidating_spend: false,
            routes_version: None,
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
    /// When false every read goes to the store (the kill switch).
    enabled: bool,
    ttls: Ttls,
    tenants: Mutex<HashMap<Uuid, Arc<Mutex<TenantEntry>>>>,
    /// Key hash -> tenant. The one thing the request path would otherwise read
    /// from Postgres; invalidation on key writes keeps revocation immediate.
    auth: Mutex<HashMap<String, (Tenant, Instant)>>,
    /// Key hashes with a background auth refresh in flight.
    auth_refreshing: Mutex<std::collections::HashSet<String>>,
    /// The last system-prompt hash per tenant, for the pre-flight repeat check.
    recent_prompts: Mutex<HashMap<Uuid, (String, Instant)>>,
    counters: Counters,
}

/// Lifetime counters for the operator-facing cache report (ITEM-092).
#[derive(Default)]
struct Counters {
    auth_hits: AtomicU64,
    auth_misses: AtomicU64,
    config_hits: AtomicU64,
    config_misses: AtomicU64,
    route_hits: AtomicU64,
    route_misses: AtomicU64,
    warm_hits: AtomicU64,
    warm_misses: AtomicU64,
    spend_hits: AtomicU64,
    spend_misses: AtomicU64,
    invalidations: AtomicU64,
}

/// Hits and misses for one cached read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheCount {
    pub hits: u64,
    pub misses: u64,
}

impl CacheCount {
    fn total(self) -> u64 {
        self.hits + self.misses
    }
    /// Share of reads served from memory; `None` before the first read.
    pub fn hit_rate(self) -> Option<f64> {
        (self.total() > 0).then(|| self.hits as f64 / self.total() as f64)
    }
}

/// A snapshot of the hot-path caches since process start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub auth: CacheCount,
    pub config: CacheCount,
    pub route: CacheCount,
    pub warm: CacheCount,
    pub spend: CacheCount,
    pub invalidations: u64,
    pub tenants: usize,
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

    /// The kill switch: no caching, every read goes to the store. Used by
    /// `HOTPATH_CACHE=off` to take the cache out of the equation instantly.
    pub fn uncached() -> Self {
        let mut hotpath = Self::new();
        hotpath.enabled = false;
        hotpath
    }

    pub fn with_ttls(ttls: Ttls) -> Self {
        Self {
            enabled: true,
            ttls,
            tenants: Mutex::new(HashMap::new()),
            auth: Mutex::new(HashMap::new()),
            auth_refreshing: Mutex::new(std::collections::HashSet::new()),
            recent_prompts: Mutex::new(HashMap::new()),
            counters: Counters::default(),
        }
    }

    /// Drop everything cached for a tenant. Call after any write that changes
    /// its controls, providers, models or routing.
    pub fn invalidate(&self, tenant_id: Uuid) {
        self.counters.invalidations.fetch_add(1, Ordering::Relaxed);
        self.tenants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&tenant_id);
        self.auth
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, (tenant, _)| tenant.id != tenant_id);
    }

    /// A snapshot for the metrics page (ITEM-092). Lifetime counters, so the
    /// report says "since process start" rather than implying a time window.
    pub fn stats(&self) -> CacheStats {
        let count = |hits: &AtomicU64, misses: &AtomicU64| CacheCount {
            hits: hits.load(Ordering::Relaxed),
            misses: misses.load(Ordering::Relaxed),
        };
        CacheStats {
            auth: count(&self.counters.auth_hits, &self.counters.auth_misses),
            config: count(&self.counters.config_hits, &self.counters.config_misses),
            route: count(&self.counters.route_hits, &self.counters.route_misses),
            warm: count(&self.counters.warm_hits, &self.counters.warm_misses),
            spend: count(&self.counters.spend_hits, &self.counters.spend_misses),
            invalidations: self.counters.invalidations.load(Ordering::Relaxed),
            tenants: self
                .tenants
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len(),
        }
    }

    /// Drop one key's cached identity. Used when only the key id is known (the
    /// operator revoke path); the tenant's other caches are unaffected.
    pub fn invalidate_key(&self, key_id: Uuid) {
        self.counters.invalidations.fetch_add(1, Ordering::Relaxed);
        self.auth
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, (tenant, _)| tenant.api_key_id != Some(key_id));
    }

    /// Resolve a key hash to its tenant. Fresh entries are served from memory;
    /// a stale one is served *and* refreshed in the background, so a request
    /// never waits on the lookup. Revocation drops the entry (see
    /// `invalidate_key`), so a stale hit can only exist for keys nobody revoked.
    pub async fn tenant_for_key(
        &self,
        state: &Arc<AppState>,
        key_hash: &str,
    ) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>> {
        if !self.enabled {
            return state.tenant_store.tenant_by_api_key_hash(key_hash).await;
        }
        let cached = {
            let auth = self
                .auth
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            auth.get(key_hash).cloned()
        };
        if let Some((tenant, at)) = cached {
            self.counters.auth_hits.fetch_add(1, Ordering::Relaxed);
            if at.elapsed() >= self.ttls.auth {
                self.spawn_auth_refresh(state, key_hash.to_string());
            }
            return Ok(Some(tenant));
        }
        self.counters.auth_misses.fetch_add(1, Ordering::Relaxed);
        let tenant = state.tenant_store.tenant_by_api_key_hash(key_hash).await?;
        if let Some(tenant) = &tenant {
            self.auth
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(key_hash.to_string(), (tenant.clone(), Instant::now()));
        }
        Ok(tenant)
    }

    fn spawn_auth_refresh(&self, state: &Arc<AppState>, key_hash: String) {
        {
            let mut refreshing = self
                .auth_refreshing
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !refreshing.insert(key_hash.clone()) {
                return;
            }
        }
        let state = state.clone();
        tokio::spawn(async move {
            match state
                .tenant_store
                .tenant_by_api_key_hash(&key_hash)
                .await
            {
                Ok(Some(tenant)) => {
                    state
                        .hotpath
                        .auth
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .insert(key_hash.clone(), (tenant, Instant::now()));
                }
                Ok(None) => {
                    // Revoked or deleted out of band: stop serving it.
                    state
                        .hotpath
                        .auth
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .remove(&key_hash);
                }
                Err(e) => tracing::warn!("hot-path auth refresh failed: {e}"),
            }
            state
                .hotpath
                .auth_refreshing
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&key_hash);
        });
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

    /// Serve the control-plane snapshot, refreshing it in the background when it
    /// is older than the config TTL. Only the very first read (no snapshot yet)
    /// waits for storage.
    async fn refresh(
        &self,
        state: &Arc<AppState>,
        tenant_id: Uuid,
    ) -> Result<Arc<Mutex<TenantEntry>>, axum::response::Response> {
        let entry = self.entry(tenant_id);
        if !self.enabled {
            self.counters.config_misses.fetch_add(1, Ordering::Relaxed);
            self.load_config(state, tenant_id, &entry).await?;
            return Ok(entry);
        }
        let phase = {
            let e = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match e.refreshed {
                None => 0, // cold: must load before answering
                Some(at) if at.elapsed() < self.ttls.config => 1,
                Some(_) => 2, // stale: answer now, refresh off-path
            }
        };
        match phase {
            1 => {
                self.counters.config_hits.fetch_add(1, Ordering::Relaxed);
            }
            2 => {
                self.counters.config_hits.fetch_add(1, Ordering::Relaxed);
                self.spawn_config_refresh(state, tenant_id, entry.clone());
            }
            _ => {
                self.counters.config_misses.fetch_add(1, Ordering::Relaxed);
                self.load_config(state, tenant_id, &entry).await?;
            }
        }
        Ok(entry)
    }

    /// Read the controls once and install them. Never called on the request path
    /// except for the first, cold read.
    async fn load_config(
        &self,
        state: &Arc<AppState>,
        tenant_id: Uuid,
        entry: &Arc<Mutex<TenantEntry>>,
    ) -> Result<(), axum::response::Response> {
        let budget = match state.budget_store.budget_for(tenant_id).await {
            Ok(budget) => budget.map(|mut budget| {
                budget.spent_cents = 0; // spend has its own, shorter cache
                budget
            }),
            Err(e) => {
                tracing::error!("budget lookup failed: {e}");
                return Err(crate::json_error(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "budget_store_error",
                    "budget lookup failed",
                ));
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
        let version = policy.as_ref().map(|policy| policy.version);
        let mut e = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        e.refreshed = Some(Instant::now());
        e.budget = budget;
        e.policy = policy;
        e.fallbacks = fallbacks;
        // Routes survive a refresh (provider/model writes invalidate them), but a
        // new policy version changes the weights, so those decisions are redone.
        if e.routes_version != version {
            e.routes.clear();
            e.routes_version = version;
        }
        Ok(())
    }

    fn spawn_config_refresh(
        &self,
        state: &Arc<AppState>,
        tenant_id: Uuid,
        entry: Arc<Mutex<TenantEntry>>,
    ) {
        {
            let mut e = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if e.refreshing {
                return;
            }
            e.refreshing = true;
        }
        let state = state.clone();
        tokio::spawn(async move {
            if state
                .hotpath
                .load_config(&state, tenant_id, &entry)
                .await
                .is_err()
            {
                tracing::warn!("hot-path config refresh failed for {tenant_id}");
            }
            entry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .refreshing = false;
        });
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
        if self.enabled {
            let cached = {
                let e = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                e.spend
            };
            if let Some((value, at)) = cached {
                self.counters.spend_hits.fetch_add(1, Ordering::Relaxed);
                if at.elapsed() >= self.ttls.spend {
                    self.spawn_spend_refresh(state, tenant_id, entry.clone());
                }
                return Ok(value);
            }
        }
        self.counters.spend_misses.fetch_add(1, Ordering::Relaxed);
        self.load_spend(state, tenant_id, entry).await
    }

    async fn load_spend(
        &self,
        state: &Arc<AppState>,
        tenant_id: Uuid,
        entry: &Arc<Mutex<TenantEntry>>,
    ) -> Result<i64, axum::response::Response> {
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

    fn spawn_spend_refresh(
        &self,
        state: &Arc<AppState>,
        tenant_id: Uuid,
        entry: Arc<Mutex<TenantEntry>>,
    ) {
        {
            let mut e = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if e.revalidating_spend {
                return;
            }
            e.revalidating_spend = true;
        }
        let state = state.clone();
        tokio::spawn(async move {
            if state
                .hotpath
                .load_spend(&state, tenant_id, &entry)
                .await
                .is_err()
            {
                tracing::warn!("hot-path spend refresh failed for {tenant_id}");
            }
            entry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .revalidating_spend = false;
        });
    }

    /// ITEM-090: record this request's system-prompt hash and say whether the
    /// previous request from the tenant had the same one (inside `window`).
    pub fn note_prompt(&self, tenant_id: Uuid, hash: &str, window: std::time::Duration) -> bool {
        let mut recent = self
            .recent_prompts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let repeated = recent
            .get(&tenant_id)
            .is_some_and(|(last, at)| last == hash && at.elapsed() < window);
        recent.insert(tenant_id, (hash.to_string(), Instant::now()));
        repeated
    }

    /// ITEM-088: may this tenant's raw payloads be stored? Fail-open (true) when
    /// the config cannot be read, matching the pre-consent behaviour.
    pub async fn capture_enabled(&self, state: &Arc<AppState>, tenant_id: Uuid) -> bool {
        match self.refresh(state, tenant_id).await {
            Ok(entry) => entry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .budget
                .as_ref()
                .is_none_or(|budget| budget.capture_payloads),
            Err(_) => true,
        }
    }

    /// ITEM-090: is the pre-flight lint pass on for this tenant? Fail-open, like
    /// the other switches: an unreadable config keeps the advisory pass.
    pub async fn lint_enabled(&self, state: &Arc<AppState>, tenant_id: Uuid) -> bool {
        match self.refresh(state, tenant_id).await {
            Ok(entry) => entry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .budget
                .as_ref()
                .is_none_or(|budget| budget.lint_enabled),
            Err(_) => true,
        }
    }

    /// ITEM-094: may a provider that speaks the other surface be reached through
    /// the translation seam? Fail-open, like the other switches.
    pub async fn translation_enabled(&self, state: &Arc<AppState>, tenant_id: Uuid) -> bool {
        match self.refresh(state, tenant_id).await {
            Ok(entry) => entry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .budget
                .as_ref()
                .is_none_or(|budget| budget.translate_surfaces),
            Err(_) => true,
        }
    }

    /// ITEM-096: detection and redaction modes for this tenant. Detection is on
    /// unless the config says otherwise; the redactions are opt-in.
    pub(crate) async fn pii_plan(&self, state: &Arc<AppState>, tenant_id: Uuid) -> crate::proxy::PiiPlan {
        match self.refresh(state, tenant_id).await {
            Ok(entry) => {
                let entry = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                match entry.budget.as_ref() {
                    Some(budget) => crate::proxy::PiiPlan {
                        detect: budget.pii_detection,
                        redact_storage: budget.pii_redact_storage,
                        redact_wire: budget.pii_redact_wire,
                    },
                    None => crate::proxy::PiiPlan {
                        detect: true,
                        ..Default::default()
                    },
                }
            }
            Err(_) => crate::proxy::PiiPlan {
                detect: true,
                ..Default::default()
            },
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
        if !self.enabled {
            self.counters.route_misses.fetch_add(1, Ordering::Relaxed);
            let route = resolve_route(state, tenant, Some(model), surface, weights).await?;
            let disabled = state
                .model_store
                .is_disabled(route.provider_id, model)
                .await
                .unwrap_or(false);
            return Ok(Decision { route, disabled });
        }
        let entry = self.entry(tenant.id);
        if let Some(hit) = entry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .routes
            .get(&key)
        {
            self.counters.route_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(hit.clone());
        }
        self.counters.route_misses.fetch_add(1, Ordering::Relaxed);
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
        if !self.enabled {
            self.counters.warm_misses.fetch_add(1, Ordering::Relaxed);
            return state
                .affinity_store
                .warm_target(tenant_id, hash)
                .await
                .unwrap_or(None);
        }
        let entry = self.entry(tenant_id);
        let cached = {
            let e = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            e.warm
                .get(hash)
                .filter(|(_, at)| at.elapsed() < self.ttls.warm)
                .map(|(target, _)| target.clone())
        };
        if let Some(cached) = cached {
            self.counters.warm_hits.fetch_add(1, Ordering::Relaxed);
            return cached;
        }
        self.counters.warm_misses.fetch_add(1, Ordering::Relaxed);
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
