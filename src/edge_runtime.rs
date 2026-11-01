//! Edge runtime (ITEM-070): run the proxy next to the customer's own model.
//!
//! The edge serves the OpenAI-compatible surface on the customer's network
//! against a **local** provider, keeps the cloud as the control plane (it polls
//! `GET /v1/edge/config` and re-syncs on change), and ships batched telemetry
//! outbound via `POST /v1/edge/traces`. Inference payloads never leave the
//! network; only small config/telemetry traffic does.
//!
//! All stores are local: the synced config is the source of truth for auth
//! (client key hashes), routing, budget, guard and the policy. The provider
//! credential is a local secret.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::affinity::InMemoryAffinityStore;
use crate::budget::{BudgetStore, ConfigUpdate, TenantBudget};
use crate::models::{ModelSetting, ModelStore};
use crate::policy::{Policy, PolicyStore, StoreResult};
use crate::providers::{InMemoryProviderStore, NewProvider, ProviderStore};
use crate::routing::RoutingStore;
use crate::telemetry::{telemetry_worker, TraceMessage, TraceSink};
use crate::tenant::{ApiKeyInfo, Tenant, TenantStore};
use crate::{build_router, AppState};

/// The tenant config the cloud hands the node (no provider secrets).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct EdgeConfig {
    #[serde(default)]
    pub tenant_id: Option<Uuid>,
    #[serde(default)]
    pub key_hashes: Vec<String>,
    #[serde(default)]
    pub fallbacks: Vec<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub policy: Option<Policy>,
    #[serde(default)]
    pub budget: Option<EdgeBudget>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct EdgeBudget {
    #[serde(default)]
    pub limit_cents: i64,
    #[serde(default)]
    pub spent_cents: i64,
    #[serde(default)]
    pub cheap_fallback_model: Option<String>,
    #[serde(default = "default_true")]
    pub guard_enabled: bool,
    #[serde(default = "default_true")]
    pub router_enabled: bool,
    #[serde(default = "default_threshold")]
    pub router_threshold: f64,
    /// ITEM-081: the tenant's tier pairs, synced so the node routes like the
    /// cloud does.
    #[serde(default)]
    pub tier_pairs: Vec<crate::router::TierPair>,
    /// ITEM-088: store raw payloads at all.
    #[serde(default = "default_true")]
    pub capture_payloads: bool,
    #[serde(default = "default_retention")]
    pub payload_retention_days: i64,
}

fn default_retention() -> i64 {
    30
}

fn default_threshold() -> f64 {
    crate::router::DOWNGRADE_THRESHOLD
}

fn default_true() -> bool {
    true
}

/// `{ version, config }` — the wire shape of `GET /v1/edge/config`.
#[derive(Deserialize)]
struct ConfigEnvelope {
    version: String,
    config: EdgeConfig,
}

/// The node's live config, read by the local stores and replaced on sync.
pub struct EdgeState {
    config: RwLock<EdgeConfig>,
}

impl EdgeState {
    pub fn new(config: EdgeConfig) -> Self {
        Self {
            config: RwLock::new(config),
        }
    }

    pub fn replace(&self, config: EdgeConfig) {
        *self.config.write().unwrap_or_else(std::sync::PoisonError::into_inner) = config;
    }

    fn read(&self) -> EdgeConfig {
        self.config
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl TenantStore for EdgeState {
    async fn tenant_by_api_key_hash(&self, api_key_hash: &str) -> StoreResult<Option<Tenant>> {
        let config = self.read();
        if !config.key_hashes.iter().any(|hash| hash == api_key_hash) {
            return Ok(None);
        }
        Ok(Some(Tenant {
            id: config.tenant_id.unwrap_or_else(Uuid::nil),
            upstream_openai_key: None,
            api_key_id: None,
            limits: crate::ratelimit::Limits::default(),
        }))
    }

    async fn tenant_by_id(&self, tenant_id: Uuid) -> StoreResult<Option<Tenant>> {
        let config = self.read();
        if config.tenant_id != Some(tenant_id) {
            return Ok(None);
        }
        Ok(Some(Tenant {
            id: tenant_id,
            upstream_openai_key: None,
            api_key_id: None,
            limits: crate::ratelimit::Limits::default(),
        }))
    }

    async fn insert_api_key(&self, _t: Uuid, _l: &str, _h: &str) -> StoreResult<Uuid> {
        Err("edge mode does not manage API keys".into())
    }
    async fn list_api_keys(&self, _t: Uuid) -> StoreResult<Vec<ApiKeyInfo>> {
        Ok(Vec::new())
    }
    async fn revoke_api_key(&self, _id: Uuid) -> StoreResult<bool> {
        Ok(false)
    }
    async fn insert_tenant(&self, _k: Option<&str>) -> StoreResult<Uuid> {
        Err("edge mode does not provision tenants".into())
    }
    async fn revoke_tenant_key(&self, _t: Uuid, _id: Uuid) -> StoreResult<bool> {
        Ok(false)
    }
    async fn set_key_spend_limit(&self, _t: Uuid, _id: Uuid, _c: i64) -> StoreResult<bool> {
        Ok(false)
    }
}

#[async_trait]
impl BudgetStore for EdgeState {
    async fn budget_for(&self, _t: Uuid) -> StoreResult<Option<TenantBudget>> {
        Ok(self.read().budget.map(|budget| TenantBudget {
            limit_cents: budget.limit_cents,
            spent_cents: budget.spent_cents,
            cheap_fallback_model: budget.cheap_fallback_model,
            guard_enabled: budget.guard_enabled,
            router_enabled: budget.router_enabled,
            router_threshold: budget.router_threshold,
            tier_pairs: budget.tier_pairs,
            capture_payloads: budget.capture_payloads,
            payload_retention_days: budget.payload_retention_days,
            // The edge runs the same advisory pass (ITEM-090).
            lint_enabled: true,
            lint_rules: crate::preflight::Rules::default().names(),
            translate_surfaces: true,
            rpm_limit: 0,
            tpm_limit: 0,
            pii_detection: true,
            pii_redact_storage: false,
            pii_redact_wire: false,
        }))
    }

    async fn guard_enabled_for(&self, _t: Uuid) -> StoreResult<bool> {
        Ok(self
            .read()
            .budget
            .map(|budget| budget.guard_enabled)
            .unwrap_or(true))
    }

    async fn update_config(&self, _t: Uuid, _u: &ConfigUpdate) -> StoreResult<()> {
        Err("edge mode does not manage config".into())
    }
}

#[async_trait]
impl RoutingStore for EdgeState {
    async fn fallbacks(&self, _t: Uuid) -> StoreResult<Vec<String>> {
        Ok(self.read().fallbacks)
    }
    async fn set_fallbacks(&self, _t: Uuid, _m: &[String]) -> StoreResult<()> {
        Err("edge mode does not manage routing".into())
    }
}

#[async_trait]
impl ModelStore for EdgeState {
    async fn settings(&self, _p: Uuid) -> StoreResult<Vec<ModelSetting>> {
        Ok(Vec::new())
    }
    async fn set_enabled(&self, _p: Uuid, _m: &str, _e: bool) -> StoreResult<()> {
        Err("edge mode does not manage models".into())
    }
    async fn set_default(&self, _p: Uuid, _m: &str) -> StoreResult<()> {
        Err("edge mode does not manage models".into())
    }
    /// The edge serves what the cloud enabled; disabled models are refused
    /// cloud-side, so nothing is disabled locally.
    async fn is_disabled(&self, _p: Option<Uuid>, _m: &str) -> StoreResult<bool> {
        Ok(false)
    }
}

#[async_trait]
impl PolicyStore for EdgeState {
    async fn active(&self, _t: Uuid) -> StoreResult<Option<Policy>> {
        Ok(self.read().policy)
    }
    async fn publish(&self, _t: Uuid, _p: Policy) -> StoreResult<Policy> {
        Err("edge mode does not publish policy".into())
    }
    async fn rollback(&self, _t: Uuid, _v: i64) -> StoreResult<Option<Policy>> {
        Ok(None)
    }
    async fn freeze(&self, _t: Uuid) -> StoreResult<()> {
        Ok(())
    }
}

/// Bound on buffered traces while the cloud is unreachable (drop oldest).
const MAX_PENDING: usize = 5_000;

/// Ships traces to the cloud control plane in batches; keeps unsent traces
/// while the cloud is unreachable and drops the oldest past the bound.
pub struct RemoteTraceSink {
    client: reqwest::Client,
    url: String,
    token: String,
    pending: Vec<Value>,
}

impl RemoteTraceSink {
    pub fn new(control_url: &str, token: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            url: format!("{}/v1/edge/traces", control_url.trim_end_matches('/')),
            token: token.to_string(),
            pending: Vec::new(),
        }
    }

    async fn ship(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let body = json!({ "traces": self.pending });
        let result = self
            .client
            .post(&self.url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&body)
            .send()
            .await;
        match result {
            Ok(response) if response.status().is_success() => {
                self.pending.clear();
                crate::metrics::record_flush();
            }
            Ok(response) if response.status() == reqwest::StatusCode::UNAUTHORIZED => {
                // The node was revoked: nothing will be accepted again.
                tracing::error!(
                    "edge node credential rejected; dropping {} buffered traces",
                    self.pending.len()
                );
                self.pending.clear();
            }
            Ok(response) => {
                tracing::warn!("edge telemetry rejected: HTTP {}", response.status());
            }
            Err(e) => tracing::warn!("edge telemetry ship failed (will retry): {e}"),
        }
        if self.pending.len() > MAX_PENDING {
            let excess = self.pending.len() - MAX_PENDING;
            self.pending.drain(..excess);
            tracing::warn!("edge telemetry buffer over {MAX_PENDING}; dropped {excess} oldest");
        }
    }
}

#[async_trait]
impl TraceSink for RemoteTraceSink {
    async fn flush(&mut self, batch: &mut Vec<TraceMessage>) {
        for msg in batch.drain(..) {
            self.pending.push(wire_trace(&msg));
        }
        self.ship().await;
    }
}

/// The cloud-side `EdgeTrace` wire shape (cost and waste are recomputed there).
fn wire_trace(msg: &TraceMessage) -> Value {
    json!({
        "id": msg.id.to_string(),
        "model": msg.request_model,
        "system_prompt_hash": msg.system_prompt_hash,
        "prompt_tokens": msg.prompt_tokens,
        "completion_tokens": msg.completion_tokens,
        "cached_prompt_tokens": msg.cached_prompt_tokens,
        "ttft_ms": msg.ttft_ms,
        "total_duration_ms": msg.total_duration_ms,
        "status_code": msg.status_code,
        "raw_prompt": msg.raw_prompt,
        "raw_completion": msg.raw_completion,
        "truncated": msg.truncated,
        "guard_event": msg.guard_event,
        "routing_event": msg.routing_event,
        "router_event": msg.router_event,
        "provider_id": msg.provider_id,
        "api_key_id": msg.api_key_id,
        "is_local": true,
        "outcome": msg.outcome.as_str(),
        "error_class": msg.error_class.map(|class| class.as_str()),
    })
}

/// Resolve a credential that may be given inline or read from a file. A file
/// wins, so the secret need not appear in `ps` or shell history.
pub fn credential(
    value: Option<String>,
    file: Option<String>,
    flag: &str,
) -> Result<String, String> {
    let resolved = match file {
        Some(path) => std::fs::read_to_string(&path)
            .map_err(|e| format!("{flag}-file {path}: {e}"))?
            .trim_end()
            .to_string(),
        None => value.unwrap_or_default(),
    };
    if resolved.is_empty() {
        return Err(format!(
            "edge mode needs {flag} <value> or {flag}-file <path>"
        ));
    }
    Ok(resolved)
}

/// How to run the node.
#[derive(Clone)]
pub struct EdgeOptions {
    pub control_url: String,
    pub node_token: String,
    /// The **local** model endpoint (e.g. `http://192.168.1.10:1234/v1`).
    pub provider_url: String,
    pub provider_key: String,
    pub provider_kind: String,
    pub port: u16,
    pub sync_interval: Duration,
}

async fn fetch_config(
    client: &reqwest::Client,
    control_url: &str,
    token: &str,
) -> Result<(String, EdgeConfig), String> {
    let url = format!("{}/v1/edge/config", control_url.trim_end_matches('/'));
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("edge node credential rejected (revoked?)".to_string());
    }
    if !response.status().is_success() {
        return Err(format!("control plane returned {}", response.status()));
    }
    let envelope: ConfigEnvelope = response.json().await.map_err(|e| e.to_string())?;
    Ok((envelope.version, envelope.config))
}

/// The models the **local** provider actually serves (`GET <provider-url>/models`),
/// falling back to the cloud catalog when the local server cannot be read — so an
/// on-prem node serves its own models, not the cloud's (ITEM-070 follow-up).
async fn local_models(
    client: &reqwest::Client,
    options: &EdgeOptions,
    fallback: &[String],
) -> Vec<String> {
    let provider = crate::providers::ResolvedProvider {
        last_check_ok: None,
        id: Uuid::nil(),
        kind: options.provider_kind.clone(),
        base_url: options.provider_url.clone(),
        api_key: options.provider_key.clone(),
        metadata: Default::default(),
    };
    match crate::providers::fetch_models(client, &provider).await {
        Ok(models) if !models.is_empty() => models,
        Ok(_) => fallback.to_vec(),
        Err(e) => {
            tracing::warn!("edge local model discovery failed ({e}); using the cloud catalog");
            fallback.to_vec()
        }
    }
}

/// Run the node: sync config, serve locally, ship telemetry. Never blocks on the
/// cloud — an unreachable control plane leaves the node serving the last config.
pub async fn run(options: EdgeOptions) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .build()?;
    let (mut version, config) = fetch_config(&client, &options.control_url, &options.node_token)
        .await
        .map_err(|e| format!("initial edge config fetch failed: {e}"))?;
    let tenant_id = config
        .tenant_id
        .ok_or("control plane did not provide a tenant id")?;

    // Local provider: the credential is a local secret, never fetched.
    let provider_store = Arc::new(InMemoryProviderStore::default());
    let provider_id = provider_store
        .insert(
            tenant_id,
            &NewProvider {
                kind: options.provider_kind.clone(),
                name: "Local provider".to_string(),
                base_url: options.provider_url.clone(),
                key: options.provider_key.clone(),
                metadata: Default::default(),
                spend_limit_cents: 0,
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    let models = local_models(&client, &options, &config.models).await;
    provider_store
        .record_models(provider_id, &models)
        .await
        .map_err(|e| e.to_string())?;

    let edge_state = Arc::new(EdgeState::new(config));

    // Telemetry: batch locally, ship to the cloud.
    let (telemetry_tx, telemetry_rx) = mpsc::channel::<TraceMessage>(10_000);
    tokio::spawn(telemetry_worker(
        telemetry_rx,
        Box::new(RemoteTraceSink::new(&options.control_url, &options.node_token)),
        Duration::from_millis(500),
        100,
        HashMap::new(),
    ));

    let state = Arc::new(AppState {
        http_client: client.clone(),
        upstream_url: String::new(),
        tenant_store: edge_state.clone(),
        provider_store: provider_store.clone(),
        model_store: edge_state.clone(),
        routing_store: edge_state.clone(),
        budget_store: edge_state.clone(),
        policy_store: edge_state.clone(),
        hotpath: Arc::new(crate::hotpath::HotPath::new()),
        affinity_store: Arc::new(InMemoryAffinityStore::default()),
        edge_store: Arc::new(crate::edge::InMemoryEdgeNodeStore::default()),
        telemetry_tx,
        operator_key: None,
        accounts: None,
        db: sqlx::PgPool::connect_lazy("postgres://edge:edge@localhost:5432/edge")?,
    });

    // Config sync: re-apply on change; a revoked credential stops the loop.
    {
        let client = client.clone();
        let edge_state = edge_state.clone();
        let provider_store = provider_store.clone();
        let options = options.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(options.sync_interval).await;
                match fetch_config(&client, &options.control_url, &options.node_token).await {
                    Ok((next_version, config)) => {
                        if next_version == version {
                            continue;
                        }
                        let models = local_models(&client, &options, &config.models).await;
                        provider_store
                            .record_models(provider_id, &models)
                            .await
                            .ok();
                        edge_state.replace(config);
                        version = next_version;
                        tracing::info!("edge config synced (version {version})");
                    }
                    Err(e) => {
                        tracing::warn!("edge config sync failed (serving last config): {e}");
                        if e.contains("revoked") {
                            tracing::error!("edge node revoked; stopping config sync");
                            break;
                        }
                    }
                }
            }
        });
    }

    let listener = TcpListener::bind(("0.0.0.0", options.port)).await?;
    tracing::info!(
        "inferon edge node listening on port {} (tenant {tenant_id}, provider {})",
        options.port,
        options.provider_url
    );
    axum::serve(listener, build_router(state)).await?;
    Ok(())
}

#[cfg(test)]
mod tests;
