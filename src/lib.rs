pub mod alerts;
pub mod budget;
pub mod guard;
use guard::GuardReason;
pub mod mock_github;
pub mod mock_upstream;
pub mod cost;
pub mod auth;
pub mod dashboard;
pub mod linter;
pub mod router;
pub mod session;

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header::{AUTHORIZATION, CONTENT_TYPE}, HeaderValue, StatusCode},
    response::Response,
    routing::{get, post},
    Router,
};
use futures_util::{Stream, StreamExt};
use reqwest::Client;
use serde_json::Value;
use sqlx::types::Json;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use uuid::Uuid;

const MAX_REQUEST_BYTES: usize = 10 * 1024 * 1024;

/// A resolved tenant. The upstream key is Inferon-held (the tenant's master
/// key); client-presented API keys are only ever stored/compared as SHA-256.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tenant {
    pub id: Uuid,
    pub upstream_openai_key: Option<String>,
}

/// Metadata for one `api_keys` row (ITEM-034). Only metadata is exposed — key
/// hashes and key material are never returned by the store. Timestamps render
/// as text so the store stays chrono-free.
#[derive(Clone, Debug)]
pub struct ApiKeyInfo {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub label: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
    pub last_used_at: Option<String>,
}

impl ApiKeyInfo {
    pub fn revoked(&self) -> bool {
        self.revoked_at.is_some()
    }
}

/// Lookup + key-management seam over tenants and their `api_keys` rows.
/// InMemoryTenantStore backs unit tests; PostgresTenantStore is the production
/// store (schema: tenants + api_keys; auth resolves active keys only — a
/// revoked key stops authenticating immediately, ITEM-034 AC2).
#[async_trait::async_trait]
pub trait TenantStore: Send + Sync {
    /// Resolve a tenant from the SHA-256 hash of a client API key. `None` for
    /// an unknown hash or a revoked key.
    async fn tenant_by_api_key_hash(
        &self,
        api_key_hash: &str,
    ) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>>;

    /// Resolve a tenant by id (used by key-mint validation).
    async fn tenant_by_id(
        &self,
        tenant_id: Uuid,
    ) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>>;

    /// Mint a key row: stores only the hash, returns the new key id. Errors
    /// when the tenant does not exist.
    async fn insert_api_key(
        &self,
        tenant_id: Uuid,
        label: &str,
        key_hash: &str,
    ) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>>;

    /// List a tenant's key metadata, oldest first.
    async fn list_api_keys(
        &self,
        tenant_id: Uuid,
    ) -> Result<Vec<ApiKeyInfo>, Box<dyn std::error::Error + Send + Sync>>;

    /// Revoke a key by id (idempotent). Returns true when a key was actually
    /// revoked, false when no such active key exists.
    async fn revoke_api_key(
        &self,
        key_id: Uuid,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>>;

    /// Create a tenant row (ITEM-036 self-serve provisioning). Returns the new
    /// tenant id. `upstream_key` is the tenant's master upstream key — `None`
    /// for a keyless tenant.
    async fn insert_tenant(
        &self,
        upstream_key: Option<&str>,
    ) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>>;

    /// Revoke a key only when it belongs to `tenant_id` (ITEM-038 AC2: a
    /// tenant scoped path can never revoke a key it does not own). True when a
    /// key was actually revoked.
    async fn revoke_tenant_key(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>>;
}

#[derive(Clone)]
struct InMemoryApiKey {
    id: Uuid,
    tenant_id: Uuid,
    label: String,
    key_hash: String,
    revoked: bool,
}

#[derive(Default)]
pub struct InMemoryTenantStore {
    tenants: std::sync::Mutex<HashMap<Uuid, Tenant>>,
    keys: std::sync::Mutex<Vec<InMemoryApiKey>>,
}

impl Clone for InMemoryTenantStore {
    fn clone(&self) -> Self {
        Self {
            tenants: std::sync::Mutex::new(
                self.tenants.lock().unwrap_or_else(|p| p.into_inner()).clone(),
            ),
            keys: std::sync::Mutex::new(
                self.keys.lock().unwrap_or_else(|p| p.into_inner()).clone(),
            ),
        }
    }
}

impl InMemoryTenantStore {
    /// Legacy shape: one active key per (hash → tenant) entry — matches the
    /// pre-ITEM-034 single-column model most tests seed with.
    pub fn new(by_key_hash: HashMap<String, Tenant>) -> Self {
        let store = Self::default();
        for (hash, tenant) in by_key_hash {
            let tenant_id = tenant.id;
            store.tenants().insert(tenant_id, tenant);
            store.keys().push(InMemoryApiKey {
                id: Uuid::new_v4(),
                tenant_id,
                label: "seeded".to_string(),
                key_hash: hash,
                revoked: false,
            });
        }
        store
    }

    fn tenants(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Tenant>> {
        self.tenants.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn keys(&self) -> std::sync::MutexGuard<'_, Vec<InMemoryApiKey>> {
        self.keys.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn now_text() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default()
    }
}

#[async_trait::async_trait]
impl TenantStore for InMemoryTenantStore {
    async fn tenant_by_api_key_hash(
        &self,
        api_key_hash: &str,
    ) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>> {
        let found = self
            .keys()
            .iter()
            .find(|k| !k.revoked && k.key_hash == api_key_hash)
            .map(|k| k.tenant_id);
        match found {
            Some(id) => Ok(self.tenants().get(&id).cloned()),
            None => Ok(None),
        }
    }

    async fn tenant_by_id(
        &self,
        tenant_id: Uuid,
    ) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.tenants().get(&tenant_id).cloned())
    }

    async fn insert_api_key(
        &self,
        tenant_id: Uuid,
        label: &str,
        key_hash: &str,
    ) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
        if !self.tenants().contains_key(&tenant_id) {
            return Err("tenant not found".into());
        }
        let id = Uuid::new_v4();
        self.keys().push(InMemoryApiKey {
            id,
            tenant_id,
            label: label.to_string(),
            key_hash: key_hash.to_string(),
            revoked: false,
        });
        Ok(id)
    }

    async fn list_api_keys(
        &self,
        tenant_id: Uuid,
    ) -> Result<Vec<ApiKeyInfo>, Box<dyn std::error::Error + Send + Sync>> {
        let now = Self::now_text();
        let mut keys: Vec<ApiKeyInfo> = self
            .keys()
            .iter()
            .filter(|k| k.tenant_id == tenant_id)
            .map(|k| ApiKeyInfo {
                id: k.id,
                tenant_id: k.tenant_id,
                label: k.label.clone(),
                created_at: now.clone(),
                revoked_at: if k.revoked { Some(now.clone()) } else { None },
                last_used_at: None,
            })
            .collect();
        // Oldest first (in-memory keys are appended in mint order).
        keys.sort_by_key(|k| k.created_at.clone());
        Ok(keys)
    }

    async fn revoke_api_key(
        &self,
        key_id: Uuid,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let mut keys = self.keys();
        if let Some(k) = keys.iter_mut().find(|k| k.id == key_id && !k.revoked) {
            k.revoked = true;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn insert_tenant(
        &self,
        upstream_key: Option<&str>,
    ) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
        let id = Uuid::new_v4();
        self.tenants().insert(
            id,
            Tenant {
                id,
                upstream_openai_key: upstream_key.map(str::to_string),
            },
        );
        Ok(id)
    }

    async fn revoke_tenant_key(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let mut keys = self.keys();
        let hit = keys
            .iter_mut()
            .find(|k| k.id == key_id && k.tenant_id == tenant_id && !k.revoked);
        match hit {
            Some(k) => {
                k.revoked = true;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

pub struct PostgresTenantStore {
    pool: sqlx::PgPool,
}

impl PostgresTenantStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl TenantStore for PostgresTenantStore {
    async fn tenant_by_api_key_hash(
        &self,
        api_key_hash: &str,
    ) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>> {
        let row = sqlx::query_as::<_, (Uuid, Option<String>)>(
            "SELECT t.id, t.upstream_openai_key
             FROM api_keys k JOIN tenants t ON t.id = k.tenant_id
             WHERE k.key_hash = $1 AND k.revoked_at IS NULL",
        )
        .bind(api_key_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id, upstream_openai_key)| Tenant {
            id,
            upstream_openai_key,
        }))
    }

    async fn tenant_by_id(
        &self,
        tenant_id: Uuid,
    ) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>> {
        let row = sqlx::query_as::<_, (Uuid, Option<String>)>(
            "SELECT id, upstream_openai_key FROM tenants WHERE id = $1",
        )
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id, upstream_openai_key)| Tenant {
            id,
            upstream_openai_key,
        }))
    }

    async fn insert_api_key(
        &self,
        tenant_id: Uuid,
        label: &str,
        key_hash: &str,
    ) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO api_keys (tenant_id, label, key_hash)
             VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(tenant_id)
        .bind(label)
        .bind(key_hash)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn list_api_keys(
        &self,
        tenant_id: Uuid,
    ) -> Result<Vec<ApiKeyInfo>, Box<dyn std::error::Error + Send + Sync>> {
        let rows = sqlx::query_as::<_, (Uuid, Uuid, String, String, Option<String>, Option<String>)>(
            "SELECT id, tenant_id, label, created_at::text, revoked_at::text, last_used_at::text
             FROM api_keys WHERE tenant_id = $1 ORDER BY created_at",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, tenant_id, label, created_at, revoked_at, last_used_at)| ApiKeyInfo {
                id,
                tenant_id,
                label,
                created_at,
                revoked_at,
                last_used_at,
            })
            .collect())
    }

    async fn revoke_api_key(
        &self,
        key_id: Uuid,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let result = sqlx::query("UPDATE api_keys SET revoked_at = NOW() WHERE id = $1 AND revoked_at IS NULL")
            .bind(key_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn insert_tenant(
        &self,
        upstream_key: Option<&str>,
    ) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO tenants (upstream_openai_key) VALUES ($1) RETURNING id",
        )
        .bind(upstream_key)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn revoke_tenant_key(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let result = sqlx::query(
            "UPDATE api_keys SET revoked_at = NOW()
             WHERE id = $1 AND tenant_id = $2 AND revoked_at IS NULL",
        )
        .bind(key_id)
        .bind(tenant_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

/// One completed request, queued for the batch telemetry worker (ITEM-005).
/// The worker fills `waste_detected` and `upstream_cost_usd` (ITEM-006) before
/// handing the message to the sink.
#[derive(Clone, Debug)]
pub struct TraceMessage {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub request_model: String,
    pub system_prompt_hash: Option<String>,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    /// Provider-reported prompt tokens served from cache
    /// (`usage.prompt_tokens_details.cached_tokens`), when the provider sends it.
    pub cached_prompt_tokens: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub total_duration_ms: i64,
    pub status_code: u16,
    pub raw_prompt: Option<Value>,
    pub raw_completion: Option<Value>,
    pub waste_detected: Option<Value>,
    pub upstream_cost_usd: rust_decimal::Decimal,
    /// True when a streaming response ended before the upstream finished
    /// (client abort or mid-stream error) — token counts are then best-effort.
    pub truncated: bool,
    /// Schema-guard event (ITEM-025): present only when the guard fired on a
    /// guarded request (cheap tier + schema contract). Unguarded traffic keeps
    /// this None — the telemetry is additive only (ITEM-026 AC4).
    pub guard_event: Option<Value>,
}

/// Flush seam for the telemetry worker: batches of TraceMessages are handed
/// to a sink exactly once per flush (one DB transaction for PostgresTraceSink).
#[async_trait::async_trait]
pub trait TraceSink: Send {
    async fn flush(&mut self, batch: &mut Vec<TraceMessage>);
}

pub async fn load_system_prompt_counts(pool: &sqlx::PgPool) -> Result<HashMap<String, u32>, sqlx::Error> {
    let rows: Vec<(String, i32)> =
        sqlx::query_as("SELECT hash, count FROM system_prompt_counts")
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(hash, count)| (hash, count.max(0) as u32)).collect())
}

pub struct PostgresTraceSink {
    pool: sqlx::PgPool,
}

impl PostgresTraceSink {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl TraceSink for PostgresTraceSink {
    async fn flush(&mut self, batch: &mut Vec<TraceMessage>) {
        let mut tx = match self.pool.begin().await {
            Ok(tx) => tx,
            Err(e) => {
                tracing::error!("telemetry: cannot begin transaction, dropping {} traces: {e}", batch.len());
                batch.clear();
                return;
            }
        };
        for msg in batch.drain(..) {
            let result = sqlx::query(
                r#"INSERT INTO traces (
                    id, tenant_id, request_model, system_prompt_hash,
                    prompt_tokens, completion_tokens, time_to_first_token_ms,
                    total_duration_ms, status_code, raw_prompt, raw_completion,
                    upstream_cost_usd, waste_detected, truncated, guard_event
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)"#,
            )
            .bind(msg.id)
            .bind(msg.tenant_id)
            .bind(&msg.request_model)
            .bind(msg.system_prompt_hash.as_deref())
            .bind(msg.prompt_tokens)
            .bind(msg.completion_tokens)
            .bind(msg.ttft_ms.map(|t| t as i32))
            .bind(msg.total_duration_ms as i32)
            .bind(i32::from(msg.status_code))
            .bind(msg.raw_prompt.as_ref().map(Json))
            .bind(msg.raw_completion.as_ref().map(Json))
            .bind(msg.upstream_cost_usd)
            .bind(msg.waste_detected.as_ref().map(Json))
            .bind(msg.truncated)
            .bind(msg.guard_event.as_ref().map(Json))
            .execute(&mut *tx)
            .await;
            if let Err(e) = result {
                tracing::error!("telemetry: trace insert failed, rolling back batch: {e}");
                return; // tx dropped -> rollback keeps the flush atomic (AC6)
            }
            // Persist the per-hash occurrence count (ITEM-010) in the same
            // transaction, so rule B repeat counts survive restarts.
            if let Some(hash) = &msg.system_prompt_hash {
                let count_result = sqlx::query(
                    "INSERT INTO system_prompt_counts (hash, count) VALUES ($1, 1)
                     ON CONFLICT (hash) DO UPDATE SET count = system_prompt_counts.count + 1",
                )
                .bind(hash)
                .execute(&mut *tx)
                .await;
                if let Err(e) = count_result {
                    tracing::error!("telemetry: count upsert failed, rolling back batch: {e}");
                    return;
                }
            }
            // Accrue actual spend toward the tenant budget (ITEM-016), same
            // transaction. No-op when the tenant has no configuration row.
            let cents = cost::cost_to_cents(msg.upstream_cost_usd);
            if cents > 0 {
                let spend_result = sqlx::query(
                    "UPDATE tenant_configurations
                     SET current_spend_cents = current_spend_cents + $1
                     WHERE tenant_id = $2",
                )
                .bind(cents)
                .bind(msg.tenant_id)
                .execute(&mut *tx)
                .await;
                if let Err(e) = spend_result {
                    tracing::error!("telemetry: spend accrual failed, rolling back batch: {e}");
                    return;
                }
            }
        }
        if let Err(e) = tx.commit().await {
            tracing::error!("telemetry: commit failed: {e}");
        }
    }
}

/// Background worker: batches TraceMessages and flushes them to the sink every
/// `flush_interval` or every `batch_size` items, whichever comes first. DB I/O
/// happens only here, never on the hot request path (AC2).
/// `initial_counts` seeds the per-hash repeat counter from persisted state so
/// linter rule B keeps counting across restarts (ITEM-010).
pub async fn telemetry_worker(
    mut rx: mpsc::Receiver<TraceMessage>,
    mut sink: Box<dyn TraceSink>,
    flush_interval: Duration,
    batch_size: usize,
    initial_counts: HashMap<String, u32>,
) {
    let mut buffer: Vec<TraceMessage> = Vec::with_capacity(batch_size);
    let mut interval = tokio::time::interval(flush_interval);
    let mut hash_counts: HashMap<String, u32> = initial_counts;
    loop {
        tokio::select! {
            msg = rx.recv() => match msg {
                Some(mut msg) => {
                    let repeat_count = match &msg.system_prompt_hash {
                        Some(hash) => {
                            let count = hash_counts.entry(hash.clone()).or_insert(0);
                            *count += 1;
                            *count
                        }
                        None => 0,
                    };
                    // Enrich off the hot path (ITEM-006): lint + cost.
                    msg.waste_detected = linter::lint(&msg, repeat_count);
                    msg.upstream_cost_usd = cost::calculate_cost(
                        &msg.request_model,
                        msg.prompt_tokens,
                        msg.completion_tokens,
                        msg.cached_prompt_tokens.unwrap_or(0),
                    );
                    buffer.push(msg);
                    if buffer.len() >= batch_size {
                        sink.flush(&mut buffer).await;
                    }
                }
                None => {
                    if !buffer.is_empty() {
                        sink.flush(&mut buffer).await;
                    }
                    return;
                }
            },
            _ = interval.tick() => {
                if !buffer.is_empty() {
                    sink.flush(&mut buffer).await;
                }
            }
        }
    }
}

/// Strip ASCII whitespace from both ends of a byte slice.
fn trim_ascii_ws(mut bytes: &[u8]) -> &[u8] {
    while let Some((&first, rest)) = bytes.split_first() {
        if first.is_ascii_whitespace() {
            bytes = rest;
        } else {
            break;
        }
    }
    while let Some((&last, rest)) = bytes.split_last() {
        if last.is_ascii_whitespace() {
            bytes = rest;
        } else {
            break;
        }
    }
    bytes
}

/// Token accounting parsed from an upstream response (usage object).
/// `Default` = no usage reached us (transport error / provider sent none).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub cached_tokens: Option<i64>,
}

/// Extract token counts from the last SSE data frame that carries a usage
/// object — e.g. DeepSeek's final chunk before [DONE]. This is run over the
/// already-accumulated stream bytes at stream end, so frames may be parsed
/// whole regardless of TCP fragmentation.
pub fn usage_from_sse_bytes(bytes: &[u8]) -> Option<Usage> {
    let mut last = None;
    for line in bytes.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(data) = line.strip_prefix(b"data:") else { continue };
        let data = trim_ascii_ws(data);
        if data == b"[DONE]" {
            continue;
        }
        let Ok(frame) = serde_json::from_slice::<Value>(data) else {
            continue;
        };
        if let (Some(p), Some(c)) = (
            frame["usage"]["prompt_tokens"].as_i64(),
            frame["usage"]["completion_tokens"].as_i64(),
        ) {
            last = Some(Usage {
                prompt_tokens: p as i32,
                completion_tokens: c as i32,
                cached_tokens: frame["usage"]["prompt_tokens_details"]["cached_tokens"]
                    .as_i64(),
            });
        }
    }
    last
}

/// SHA-256 hex of the first `role: system` message content (AC3).
fn system_prompt_hash(payload: &Value) -> Option<String> {
    payload["messages"]
        .as_array()?
        .iter()
        .find(|m| m["role"] == "system")
        .and_then(|m| m["content"].as_str())
        .map(tenant_hash)
}

/// Token counts from a parsed upstream JSON response (non-streaming path).
fn usage_from_json(response_json: &Option<Value>) -> Usage {
    match response_json {
        Some(json) => Usage {
            prompt_tokens: json["usage"]["prompt_tokens"].as_i64().unwrap_or(0) as i32,
            completion_tokens: json["usage"]["completion_tokens"].as_i64().unwrap_or(0) as i32,
            cached_tokens: json["usage"]["prompt_tokens_details"]["cached_tokens"].as_i64(),
        },
        None => Usage::default(),
    }
}

pub struct AppState {
    pub http_client: Client,
    pub upstream_url: String,
    pub tenant_store: Arc<dyn TenantStore>,
    pub budget_store: Arc<dyn budget::BudgetStore>,
    pub telemetry_tx: mpsc::Sender<TraceMessage>,
    /// Operator credential (env `OPERATOR_API_KEY`): grants the platform-wide
    /// dashboard views. `None` disables operator access entirely (ITEM-013).
    pub operator_key: Option<String>,
    /// Human accounts (GitHub OAuth + signed sessions, ITEM-035). `None` when
    /// OAuth is unconfigured — `/auth/login` answers 503 then (AC3).
    pub accounts: Option<Arc<auth::Accounts>>,
    pub db: sqlx::PgPool,
}

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/chat/completions", post(proxy_chat_completions))
        .route("/v1/dashboard", get(dashboard::index))
        .route("/v1/dashboard/operator", get(dashboard::operator_console))
        .route("/v1/dashboard/api/summary", get(dashboard::summary))
        .route("/v1/dashboard/api/by-model", get(dashboard::by_model))
        .route("/v1/dashboard/api/by-prompt", get(dashboard::by_prompt))
        .route("/v1/dashboard/api/waste", get(dashboard::waste))
        .route("/v1/dashboard/api/guard", get(dashboard::guard))
        .route("/v1/dashboard/api/operator/tenants", get(dashboard::operator_tenants))
        .route("/v1/dashboard/api/operator/summary", get(dashboard::operator_summary))
        .route("/v1/dashboard/api/operator/by-model", get(dashboard::operator_by_model))
        .route("/v1/dashboard/api/operator/by-prompt", get(dashboard::operator_by_prompt))
        .route("/v1/dashboard/api/operator/alerts", get(dashboard::operator_alerts))
        .route("/v1/dashboard/api/operator/guard", get(dashboard::operator_guard))
        .route(
            "/v1/dashboard/api/operator/keys",
            get(dashboard::operator_keys).post(dashboard::operator_keys_create),
        )
        .route(
            "/v1/dashboard/api/operator/keys/:key_id/revoke",
            post(dashboard::operator_key_revoke),
        )
        .route(
            "/v1/dashboard/api/keys",
            get(dashboard::tenant_keys).post(dashboard::tenant_key_create),
        )
        .route(
            "/v1/dashboard/api/keys/:key_id/revoke",
            post(dashboard::tenant_key_revoke),
        )
        .route(
            "/v1/dashboard/api/members",
            get(dashboard::tenant_members).post(dashboard::tenant_member_invite),
        )
        .route(
            "/v1/dashboard/api/members/:user_id/remove",
            post(dashboard::tenant_member_remove),
        )
        .route("/auth/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .with_state(state)
}

async fn healthz() -> &'static str {
    "ok"
}

/// Proxy a chat completion request to the configured upstream.
///
/// Authenticates the caller via `Authorization: Bearer <key>` (SHA-256 lookup
/// against the tenant store), swaps the upstream auth header to the tenant's
/// master key, then forwards: streaming requests pipe upstream bytes through
/// unchanged (zero-copy) as text/event-stream; non-streaming requests pass the
/// JSON body through with usage intact. Transport errors become 502; upstream
/// HTTP error statuses are forwarded as-is.
async fn proxy_chat_completions(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Response {
    let tenant = match authenticate(&state, req.headers()).await {
        Ok(Some(tenant)) => tenant,
        Ok(None) => {
            return json_error(
                StatusCode::UNAUTHORIZED,
                "invalid_api_key",
                "missing or invalid Authorization header",
            )
        }
        Err(e) => {
            tracing::error!("tenant lookup failed: {e}");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "tenant_store_error",
                "tenant lookup failed",
            );
        }
    };
    let tenant_id = tenant.id;

    let body_bytes = match axum::body::to_bytes(req.into_body(), MAX_REQUEST_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return json_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "request body exceeds 10MB",
            )
        }
    };
    let payload: Value = match serde_json::from_slice(&body_bytes) {
        Ok(payload) => payload,
        Err(_) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_json",
                "request body must be valid JSON",
            )
        }
    };
    let is_stream = payload["stream"].as_bool().unwrap_or(false);
    // Slice 2 (ITEM-015): heuristic downgrade decision over the client payload
    // before it is consumed into the trace metadata.
    let router_override = router::maybe_downgrade(&payload);
    let mut meta = RequestMeta {
        id: Uuid::new_v4(),
        model: payload["model"].as_str().unwrap_or("unknown").to_string(),
        system_prompt_hash: system_prompt_hash(&payload),
        raw_prompt: payload,
    };
    let Some(upstream_key) = tenant.upstream_openai_key.as_deref() else {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "tenant_misconfigured",
            "tenant has no upstream key configured",
        );
    };

    let started = Instant::now();
    // Budget breaker (ITEM-016): its fallback model wins over the router;
    // a hard-cap rejection is traced and answered 429 before any upstream call.
    let budget_override = match budget::check_budget(&state, tenant_id).await {
        Ok(model) => model,
        Err(resp) => {
            let total_ms = started.elapsed().as_millis() as i64;
            let msg = build_trace(&meta, tenant_id, 429, total_ms, Usage::default(), None, None, false);
            send_trace(&state.telemetry_tx, msg);
            return resp;
        }
    };
    let mut model_override = budget_override.or(router_override);
    let mut upstream_body = reqwest::Body::from(body_bytes);
    if let Some(model) = model_override.take() {
        meta.model = model.clone();
        match budget::apply_model_override(&meta.raw_prompt, &model) {
            Ok(bytes) => upstream_body = reqwest::Body::from(bytes),
            Err(e) => {
                tracing::error!("payload reserialization failed: {e}");
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "serialization_error",
                    "failed to reserialize the request payload",
                );
            }
        }
    }

    let upstream_request = state
        .http_client
        .post(&state.upstream_url)
        .header("Authorization", format!("Bearer {upstream_key}"))
        .header("Content-Type", "application/json")
        .body(upstream_body);

    let upstream = match upstream_request.send().await {
        Ok(response) => response,
        Err(e) => {
            tracing::error!("upstream request failed: {e}");
            let total_ms = started.elapsed().as_millis() as i64;
            let msg = build_trace(&meta, tenant_id, 502, total_ms, Usage::default(), None, None, false);
            send_trace(&state.telemetry_tx, msg);
            return json_error(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                "upstream request failed",
            );
        }
    };

    let status = upstream.status();
    if !status.is_success() {
        // Forward the upstream error as-is (keep its content-type); still trace.
        return forward_buffered(&state, &meta, tenant_id, started, upstream, None).await;
    }
    if is_stream {
        // ITEM-023 (guard): requests with a JSON-schema contract served by a
        // cheap tier are buffered end-to-end and validated before anything
        // reaches the client. Anything else keeps the zero-copy passthrough.
        // ITEM-027 AC1: the guard is ON by default; a tenant opts out via
        // tenant_configurations.guard_enabled (fail-open on read errors).
        let guarded = guard::is_guarded_request(&meta.raw_prompt, &meta.model)
            && state
                .budget_store
                .guard_enabled_for(tenant_id)
                .await
                .unwrap_or(true);
        if guarded {
            guarded_stream_response(
                &state,
                meta,
                tenant_id,
                started,
                upstream,
                upstream_key,
                state.telemetry_tx.clone(),
            )
            .await
        } else {
            stream_response(
                upstream,
                started,
                meta,
                tenant_id,
                status.as_u16(),
                state.telemetry_tx.clone(),
            )
        }
    } else {
        let json_content_type = Some(HeaderValue::from_static("application/json"));
        forward_buffered(&state, &meta, tenant_id, started, upstream, json_content_type).await
    }
}

/// Buffer an upstream body, trace the completed request, and build the
/// forwarded response. `content_type` overrides the forwarded content-type
/// when present; otherwise the upstream's own header is preserved (upstream
/// error bodies are forwarded as-is). Shared by the non-2xx and the 2xx
/// non-streaming paths.
async fn forward_buffered(
    state: &AppState,
    meta: &RequestMeta,
    tenant_id: Uuid,
    started: Instant,
    upstream: reqwest::Response,
    content_type: Option<HeaderValue>,
) -> Response {
    let status = upstream.status();
    let upstream_content_type = upstream.headers().get(CONTENT_TYPE).cloned();
    let bytes = match upstream.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("upstream body read failed: {e}");
            return json_error(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                "upstream request failed",
            );
        }
    };
    let response_json: Option<Value> = serde_json::from_slice(&bytes).ok();
    let usage = usage_from_json(&response_json);
    let total_ms = started.elapsed().as_millis() as i64;
    // Non-streaming ttft mirrors total duration; error responses have no ttft.
    let ttft_ms = status.is_success().then_some(total_ms);
    let msg = build_trace(meta, tenant_id, status.as_u16(), total_ms, usage, ttft_ms, response_json, false);
    send_trace(&state.telemetry_tx, msg);

    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = status;
    if let Some(content_type) = content_type.or(upstream_content_type) {
        response.headers_mut().insert(CONTENT_TYPE, content_type);
    }
    response
}

/// Request-derived fields shared by every trace of this request.
#[derive(Clone)]
struct RequestMeta {
    id: Uuid,
    model: String,
    system_prompt_hash: Option<String>,
    raw_prompt: Value,
}

fn build_trace(
    meta: &RequestMeta,
    tenant_id: Uuid,
    status: u16,
    total_duration_ms: i64,
    usage: Usage,
    ttft_ms: Option<i64>,
    raw_completion: Option<Value>,
    truncated: bool,
) -> TraceMessage {
    TraceMessage {
        id: meta.id,
        tenant_id,
        request_model: meta.model.clone(),
        system_prompt_hash: meta.system_prompt_hash.clone(),
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        cached_prompt_tokens: usage.cached_tokens,
        ttft_ms,
        total_duration_ms,
        status_code: status,
        raw_prompt: Some(meta.raw_prompt.clone()),
        raw_completion,
        waste_detected: None,
        upstream_cost_usd: rust_decimal::Decimal::ZERO,
        truncated,
        guard_event: None,
    }
}

/// Queue a trace without ever blocking the request path (AC5): a full channel
/// drops the trace with an error log; a closed channel logs at debug level.
fn send_trace(tx: &mpsc::Sender<TraceMessage>, msg: TraceMessage) {
    match tx.try_send(msg) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            tracing::error!("telemetry channel full; trace dropped");
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            tracing::debug!("telemetry channel closed; trace dropped");
        }
    }
}

/// Resolve the caller's tenant from the Authorization header.
/// Ok(Some(tenant)) = authenticated; Ok(None) = missing/invalid credentials;
/// Err = store failure (caller should see 500, never a misleading 401).
pub(crate) async fn authenticate(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>> {
    let Some(auth) = headers.get(AUTHORIZATION) else {
        return Ok(None);
    };
    let Ok(auth) = auth.to_str() else {
        return Ok(None);
    };
    let Some(key) = auth.strip_prefix("Bearer ") else {
        return Ok(None);
    };
    if key.is_empty() {
        return Ok(None);
    }
    state.tenant_store.tenant_by_api_key_hash(&tenant_hash(key)).await
}

/// SHA-256 hex digest of an API key — the only form of a client key that is
/// ever stored, compared, or logged (AC5).
pub(crate) fn tenant_hash(key: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(key.as_bytes()))
}

/// Key prefix for client API keys (ITEM-034 mints). Raw keys are shown once
/// at mint time; only the SHA-256 hash is persisted.
pub(crate) const CLIENT_KEY_PREFIX: &str = "sk-inferon-";

/// Mint a fresh raw client key (128 bits of randomness, UUID v4 backend).
pub(crate) fn mint_client_key() -> String {
    format!("{CLIENT_KEY_PREFIX}{}", Uuid::new_v4().simple())
}

/// OpenAI-style JSON error response.
pub(crate) fn json_error(status: StatusCode, err_type: &str, message: &str) -> Response {
    let body = serde_json::json!({"error": {"message": message, "type": err_type}}).to_string();
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

/// Pipe an upstream byte stream through as text/event-stream, emitting the
/// request trace once the stream is fully consumed. Chunks are yielded
/// immediately (zero-copy; no buffering that would break TTFT) while bytes are
/// accumulated on the side solely for the final usage-frame parse (AC4).
fn stream_response(
    upstream: reqwest::Response,
    started: Instant,
    meta: RequestMeta,
    tenant_id: Uuid,
    status: u16,
    tx: mpsc::Sender<TraceMessage>,
) -> Response {
    let byte_stream = upstream.bytes_stream();
    let stream = stream_with_trace(byte_stream, started, meta, tenant_id, status, tx);
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    response
}

/// Guarded buffered response (ITEM-023). For requests with a JSON-schema
/// contract served by a cheap tier the whole upstream body is buffered and
/// only released to the client after `[DONE]` + successful validation. A
/// client abort during the buffered phase still emits the truncated trace
/// (ITEM-009 semantics — Drop hook on `BufferedGuard`).
///
/// ITEM-025: when the flush gate rejects the cheap response (any classified
/// failure), the bad buffer is discarded and the original prompt is replayed
/// to the tenant's frontier model with their upstream key; the replacement
/// response is served to the client and the request's single trace records
/// the guard event. If the replay itself fails the request answers loudly
/// with a clean 502 (AC4) — never a silent drop.
async fn guarded_stream_response(
    state: &Arc<AppState>,
    meta: RequestMeta,
    tenant_id: Uuid,
    started: Instant,
    upstream: reqwest::Response,
    upstream_key: &str,
    tx: mpsc::Sender<TraceMessage>,
) -> Response {
    let mut guard_emit = BufferedGuard::new(tx, meta, tenant_id, started);
    let bytes = match upstream.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("guarded upstream body read failed: {e}");
            // 025: a cheap stream that dies before any bytes is the
            // "connection crash" class — fall back, don't surface it.
            return fallback_to_frontier(&mut guard_emit, state, upstream_key, GuardReason::UpstreamFailure)
                .await;
        }
    };

    // Flush gate: container integrity then schema conformance (ITEM-024).
    let schema = guard::request_schema(&guard_emit.meta.raw_prompt);
    match guard::check_guarded_sse(&bytes, schema.as_ref()) {
        Ok(()) => {
            let usage = usage_from_sse_bytes(&bytes).unwrap_or_default();
            // ttft is when the client actually sees the first byte: after the
            // buffered flush (Slice 4 latency target measures this, ITEM-027).
            guard_emit.note_guard(None, guard::guard_flushed_event(&guard_emit.meta.model));
            guard_emit.emit(usage, false);
            let mut response = Response::new(Body::from(bytes));
            *response.status_mut() = StatusCode::OK;
            response
                .headers_mut()
                .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
            response
        }
        Err(reason) => {
            tracing::warn!(
                "guard rejected a cheap-tier response: {} (reason {}) — replaying on the frontier",
                tenant_id,
                reason.label()
            );
            fallback_to_frontier(&mut guard_emit, state, upstream_key, reason).await
        }
    }
}

/// ITEM-025: transparent fallback after the guard discards a cheap response.
/// Replays the ORIGINAL prompt (`meta.raw_prompt`, pre-router/budget
/// downgrade) on the tenant's frontier model — their own key, same upstream —
/// and serves the replacement response. Exactly one trace is emitted for the
/// request (BufferedGuard is one-shot): on success it records the fallback
/// guard event and the trace's model is retargeted to the model that served
/// (the cost basis); when the replay itself fails the request answers a loud
/// clean 502 and the trace records `fallback_failed` + the reason (AC3/AC4).
async fn fallback_to_frontier(
    guard_emit: &mut BufferedGuard,
    state: &AppState,
    upstream_key: &str,
    reason: GuardReason,
) -> Response {
    let frontier = guard::fallback_model(&guard_emit.meta.raw_prompt);
    let replay_body = match budget::apply_model_override(&guard_emit.meta.raw_prompt, &frontier) {
        Ok(body) => body,
        Err(e) => {
            tracing::error!("guard fallback payload reserialization failed: {e}");
            guard_emit.emit(Usage::default(), false);
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "serialization_error",
                "failed to reserialize the request payload",
            );
        }
    };
    let replay = state
        .http_client
        .post(&state.upstream_url)
        .header("Authorization", format!("Bearer {upstream_key}"))
        .header("Content-Type", "application/json")
        .body(replay_body)
        .send()
        .await;
    let guarded_model = guard_emit.meta.model.clone();
    match replay {
        Err(e) => {
            tracing::error!("guard frontier replay failed: {e}");
            let event =
                guard::guard_event("fallback_failed", reason, &guarded_model, Some("transport error"));
            guard_emit.note_guard(Some(&frontier), event);
            guard_emit.emit(Usage::default(), false);
            json_error(
                StatusCode::BAD_GATEWAY,
                "guard_fallback_error",
                "frontier fallback request failed",
            )
        }
        Ok(res) if !res.status().is_success() => {
            let detail = format!("http {}", res.status().as_u16());
            tracing::warn!("guard frontier replay rejected: {detail}");
            let event =
                guard::guard_event("fallback_failed", reason, &guarded_model, Some(&detail));
            guard_emit.note_guard(Some(&frontier), event);
            guard_emit.emit(Usage::default(), false);
            json_error(
                StatusCode::BAD_GATEWAY,
                "guard_fallback_error",
                "frontier fallback was rejected",
            )
        }
        Ok(res) => {
            // Note the event BEFORE buffering the replay so a mid-replay
            // client abort (Drop) still records the guard event (AC3).
            let event = guard::guard_event("fallback", reason, &guarded_model, None);
            guard_emit.note_guard(Some(&frontier), event);
            let bytes = match res.bytes().await {
                Ok(bytes) => bytes,
                Err(e) => {
                    tracing::error!("guard frontier body read failed: {e}");
                    guard_emit.emit(Usage::default(), true);
                    return json_error(
                        StatusCode::BAD_GATEWAY,
                        "guard_fallback_error",
                        "frontier fallback body read failed",
                    );
                }
            };
            let usage = usage_from_sse_bytes(&bytes).unwrap_or_default();
            guard_emit.emit(usage, false);
            let mut response = Response::new(Body::from(bytes));
            *response.status_mut() = StatusCode::OK;
            response
                .headers_mut()
                .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
            response
        }
    }
}

/// Drop-guarded trace emitter for the buffered (non-streamed-to-client)
/// phase: if the client disconnects while the upstream body is still being
/// buffered the handler future is dropped, and only this guard's Drop runs —
/// emitting a truncated trace (ITEM-009 semantics preserved for the guard).
struct BufferedGuard {
    tx: mpsc::Sender<TraceMessage>,
    meta: RequestMeta,
    tenant_id: Uuid,
    started: Instant,
    emitted: Cell<bool>,
    /// ITEM-025: the model that ultimately served (the frontier replay),
    /// overriding `meta.model` on the request's trace — the cost basis.
    retarget_model: RefCell<Option<String>>,
    /// ITEM-025 AC3: guard event attached to the single trace once the guard
    /// has fired. None until `note_guard` runs — additive only.
    guard_event: RefCell<Option<Value>>,
}

impl BufferedGuard {
    fn new(
        tx: mpsc::Sender<TraceMessage>,
        meta: RequestMeta,
        tenant_id: Uuid,
        started: Instant,
    ) -> Self {
        Self {
            tx,
            meta,
            tenant_id,
            started,
            emitted: Cell::new(false),
            retarget_model: RefCell::new(None),
            guard_event: RefCell::new(None),
        }
    }

    /// ITEM-025/026: record that the guard fired on this request. `served` is
    /// the frontier model when a fallback replays it (retargets the trace's
    /// request_model to the model that ultimately serves — the cost basis),
    /// None when the cheap model itself served. Must run before any await
    /// after the guard verdict so a client abort still records the event.
    fn note_guard(&self, served: Option<&str>, event: Value) {
        *self.retarget_model.borrow_mut() = served.map(str::to_string);
        *self.guard_event.borrow_mut() = Some(event);
    }

    fn emit(&self, usage: Usage, truncated: bool) {
        if self.emitted.replace(true) {
            return;
        }
        let total_ms = self.started.elapsed().as_millis() as i64;
        let mut meta = self.meta.clone();
        if let Some(model) = self.retarget_model.borrow().clone() {
            meta.model = model;
        }
        let mut msg = build_trace(
            &meta,
            self.tenant_id,
            200,
            total_ms,
            usage,
            None,
            None,
            truncated,
        );
        msg.guard_event = self.guard_event.borrow().clone();
        send_trace(&self.tx, msg);
    }
}

impl Drop for BufferedGuard {
    fn drop(&mut self) {
        if !self.emitted.get() {
            tracing::debug!("guarded request dropped before flush; recording truncated trace");
            self.emit(Usage::default(), true);
        }
    }
}

/// One-shot trace emitter for the streaming path. Sends the trace exactly
/// once: on clean/errored completion from the stream loop, or from `Drop` when
/// the client aborts mid-stream — nothing after a dropped future's last poll
/// runs, so `Drop` is the only hook (ITEM-009). `send` is synchronous
/// (`try_send`), so dropping never blocks.
struct StreamEmit {
    tx: mpsc::Sender<TraceMessage>,
    meta: RequestMeta,
    tenant_id: Uuid,
    status: u16,
    started: Instant,
    emitted: Cell<bool>,
    ttft_ms: Cell<Option<i64>>,
}

impl StreamEmit {
    fn new(
        tx: mpsc::Sender<TraceMessage>,
        meta: RequestMeta,
        tenant_id: Uuid,
        status: u16,
        started: Instant,
    ) -> Self {
        Self {
            tx,
            meta,
            tenant_id,
            status,
            started,
            emitted: Cell::new(false),
            ttft_ms: Cell::new(None),
        }
    }

    fn note_ttft(&self, ttft_ms: i64) {
        self.ttft_ms.set(Some(ttft_ms));
    }

    fn ttft_ms(&self) -> Option<i64> {
        self.ttft_ms.get()
    }

    /// Emit the trace unless one was already sent. `truncated` marks a stream
    /// that ended before the upstream finished (mid-stream error).
    fn emit(&self, usage: Usage, truncated: bool) {
        if self.emitted.replace(true) {
            return;
        }
        let total_ms = self.started.elapsed().as_millis() as i64;
        let msg = build_trace(
            &self.meta,
            self.tenant_id,
            self.status,
            total_ms,
            usage,
            self.ttft_ms(),
            None,
            truncated,
        );
        send_trace(&self.tx, msg);
    }
}

impl Drop for StreamEmit {
    fn drop(&mut self) {
        if !self.emitted.get() {
            // Client dropped the response mid-stream: the usage frame (sent at
            // the very end by the provider) never arrived, so token counts are
            // unknown — record zeroes and flag the trace as truncated.
            tracing::debug!("stream dropped before completion; recording truncated trace");
            self.emit(Usage::default(), true);
        }
    }
}

/// Tag each upstream chunk with its arrival time (TTFT on the first one), yield
/// it immediately, and once the stream ends queue the trace with token counts
/// parsed from the final usage frame. A client abort mid-stream still emits a
/// truncated trace via StreamEmit's Drop.
fn stream_with_trace(
    byte_stream: impl Stream<Item = Result<axum::body::Bytes, reqwest::Error>>,
    started: Instant,
    meta: RequestMeta,
    tenant_id: Uuid,
    status: u16,
    tx: mpsc::Sender<TraceMessage>,
) -> impl Stream<Item = Result<axum::body::Bytes, std::io::Error>> {
    async_stream::stream! {
        let emit = StreamEmit::new(tx, meta, tenant_id, status, started);
        let mut stream = Box::pin(byte_stream);
        let mut accumulated: Vec<u8> = Vec::new();
        let mut errored = false;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    if emit.ttft_ms().is_none() {
                        emit.note_ttft(started.elapsed().as_millis() as i64);
                        tracing::debug!(ttft_ms = emit.ttft_ms().unwrap(), "first upstream chunk");
                    }
                    accumulated.extend_from_slice(&bytes);
                    yield Ok(bytes);
                }
                Err(e) => {
                    tracing::error!("upstream stream error: {e}");
                    errored = true;
                    break;
                }
            }
        }
        let usage = usage_from_sse_bytes(&accumulated).unwrap_or_default();
        emit.emit(usage, errored);
    }
}

pub fn parse_port(value: Option<&str>) -> u16 {
    value
        .and_then(|v| v.parse().ok())
        .unwrap_or(8080)
}

pub async fn ping_health(port: u16) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let Ok(mut stream) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await else {
        return false;
    };
    if stream
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .is_err()
    {
        return false;
    }
    let mut response = String::new();
    stream.read_to_string(&mut response).await.is_ok() && response.starts_with("HTTP/1.1 200")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tower::ServiceExt;

    const SSE_BODY: &str = "data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: {\"id\":\"1\",\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\ndata: [DONE]\n\n";
    /// SSE body whose single delta content is `content_json` (a JSON object
    /// double-encoded inside the delta string, like a real chat completion).
    fn delta_only_stream(content_json: &str) -> String {
        let frame = serde_json::json!({
            "id": "1",
            "choices": [{"delta": {"content": content_json}}]
        });
        format!("data: {frame}\n\ndata: [DONE]\n\n")
    }

    const SSE_BODY_NO_USAGE: &str =
        "data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: [DONE]\n\n";
    const CLIENT_KEY: &str = "sk-client-dev-key";
    const MASTER_KEY: &str = "sk-tenant-master-key";
    const TENANT_ID: &str = "11111111-1111-1111-1111-111111111111";

    fn tenant_hash(key: &str) -> String {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(key.as_bytes()))
    }

    fn seeded_store() -> InMemoryTenantStore {
        let mut by_hash = std::collections::HashMap::new();
        by_hash.insert(
            tenant_hash(CLIENT_KEY),
            Tenant {
                id: uuid::Uuid::parse_str(TENANT_ID).unwrap(),
                upstream_openai_key: Some(MASTER_KEY.to_string()),
            },
        );
        InMemoryTenantStore::new(by_hash)
    }

    struct FailingStore;

    #[async_trait::async_trait]
    impl TenantStore for FailingStore {
        async fn tenant_by_api_key_hash(
            &self,
            _api_key_hash: &str,
        ) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>> {
            Err("tenant store unavailable".into())
        }

        async fn tenant_by_id(
            &self,
            _tenant_id: Uuid,
        ) -> Result<Option<Tenant>, Box<dyn std::error::Error + Send + Sync>> {
            Err("tenant store unavailable".into())
        }

        async fn insert_api_key(
            &self,
            _tenant_id: Uuid,
            _label: &str,
            _key_hash: &str,
        ) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
            Err("tenant store unavailable".into())
        }

        async fn list_api_keys(
            &self,
            _tenant_id: Uuid,
        ) -> Result<Vec<ApiKeyInfo>, Box<dyn std::error::Error + Send + Sync>> {
            Err("tenant store unavailable".into())
        }

        async fn revoke_api_key(
            &self,
            _key_id: Uuid,
        ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
            Err("tenant store unavailable".into())
        }

        async fn insert_tenant(
            &self,
            _upstream_key: Option<&str>,
        ) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
            Err("tenant store unavailable".into())
        }

        async fn revoke_tenant_key(
            &self,
            _tenant_id: Uuid,
            _key_id: Uuid,
        ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
            Err("tenant store unavailable".into())
        }
    }

    async fn mock_upstream() -> String {
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(
                |headers: axum::http::HeaderMap, payload: axum::Json<serde_json::Value>| async move {
                    match payload["model"].as_str() {
                        Some("fail") => axum::response::Response::builder()
                            .status(500)
                            .body(Body::from("boom"))
                            .unwrap(),
                        Some("rate") => axum::response::Response::builder()
                            .status(429)
                            .body(Body::from("rate limited"))
                            .unwrap(),
                        Some("nousage") => axum::response::Response::builder()
                            .header("content-type", "text/event-stream")
                            .body(Body::from(SSE_BODY_NO_USAGE))
                            .unwrap(),
                        Some("slow") => {
                            // Two SSE frames 300ms apart so a client can abort
                            // mid-stream while the tap is waiting for frame 2.
                            let body = async_stream::stream! {
                                yield Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
                                    b"data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
                                ));
                                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                                yield Ok(axum::body::Bytes::from_static(
                                    b"data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\ndata: [DONE]\n\n",
                                ));
                            };
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(axum::body::Body::from_stream(body))
                                .unwrap()
                        }
                        Some("miderror") => {
                            // One good frame, then — after headers have surely
                            // flushed — a mid-stream failure.
                            let body = async_stream::stream! {
                                yield Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
                                    b"data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
                                ));
                                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                                yield Err(std::io::Error::new(
                                    std::io::ErrorKind::Other,
                                    "mock mid-stream failure",
                                ));
                            };
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(axum::body::Body::from_stream(body))
                                .unwrap()
                        }
                        Some("gpt-4o-mini") if {
                            // Cheap lie: frames arrive but [DONE] never does —
                            // the guard discards the buffer and falls back.
                            // Markers: "truncated" (container failure) and
                            // "boom" (fallback-failure test, ITEM-025 AC4).
                            matches!(
                                payload["messages"][0]["content"].as_str(),
                                Some("truncated" | "boom")
                            )
                        } => {
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from(
                                    "data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\ndata: {\"id\":\"1\",\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1}}\n\n",
                                ))
                                .unwrap()
                        }
                        Some("gpt-4o-mini") if {
                            // Content that satisfies the request's answer schema
                            // (well-formed JSON object) — must be flushed.
                            payload["messages"][0]["content"].as_str() == Some("valid-json")
                        } => {
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from(delta_only_stream(r#"{"answer":"42"}"#)))
                                .unwrap()
                        }
                        Some("gpt-4o-mini") if {
                            // Cheap lie: well-formed JSON that violates the
                            // answer schema ({"other":1}, missing required
                            // "answer") — must be discarded, never forwarded.
                            payload["messages"][0]["content"].as_str() == Some("wrong-shape")
                        } => {
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from(delta_only_stream(r#"{"other":1}"#)))
                                .unwrap()
                        }
                        Some("gpt-4o-mini") if {
                            // Cheap lie for the abort-during-fallback test:
                            // instant violation, so the replay starts at once.
                            payload["messages"][0]["content"].as_str() == Some("slow-fallback")
                        } => {
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from(delta_only_stream(r#"{"other":1}"#)))
                                .unwrap()
                        }
                        Some("gpt-4o-mini") if {
                            // Slow guarded stream: first frame, 300ms gap, then
                            // [DONE] — lets a test abort mid-buffer.
                            payload["messages"][0]["content"].as_str() == Some("guarded-slow")
                        } => {
                            let body = async_stream::stream! {
                                yield Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
                                    b"data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
                                ));
                                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                                yield Ok(axum::body::Bytes::from_static(
                                    b"data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\ndata: [DONE]\n\n",
                                ));
                            };
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(axum::body::Body::from_stream(body))
                                .unwrap()
                        }
                        Some("deepseek-v4-pro") if {
                            // Fallback failure (ITEM-025 AC4): the frontier
                            // replay itself answers 5xx — loud and clean.
                            payload["messages"][0]["content"].as_str() == Some("boom")
                        } => {
                            axum::response::Response::builder()
                                .status(500)
                                .body(Body::from("boom"))
                                .unwrap()
                        }
                        Some("deepseek-v4-pro") if {
                            // Default frontier replay (cheap original escalated
                            // to the deployment frontier): honours the contract.
                            let marker = payload["messages"][0]["content"].as_str();
                            matches!(marker, Some("wrong-shape" | "truncated"))
                        } => {
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from(delta_only_stream(r#"{"answer":"42"}"#)))
                                .unwrap()
                        }
                        Some("deepseek-v4-pro") if {
                            // Slow frontier replay: first frame, 300ms gap,
                            // then [DONE] — lets a test abort mid-replay.
                            payload["messages"][0]["content"].as_str() == Some("slow-fallback")
                        } => {
                            let body = async_stream::stream! {
                                yield Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
                                    b"data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
                                ));
                                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                                yield Ok(axum::body::Bytes::from_static(
                                    b"data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\ndata: [DONE]\n\n",
                                ));
                            };
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(axum::body::Body::from_stream(body))
                                .unwrap()
                        }
                        Some("gpt-4o") if {
                            // Frontier replay when the tenant's original request
                            // (pre-downgrade) was gpt-4o: honours the contract.
                            let marker = payload["messages"][0]["content"].as_str();
                            matches!(marker, Some("wrong-shape" | "truncated"))
                        } => {
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from(delta_only_stream(r#"{"answer":"42"}"#)))
                                .unwrap()
                        }
                        Some("nodone") => {
                            // Valid JSON frames but no [DONE] — used to prove
                            // unguarded traffic pipes a truncated body through
                            // unchanged (guard never engages without a schema).
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from(
                                    "data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\ndata: {\"id\":\"1\",\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1}}\n\n",
                                ))
                                .unwrap()
                        }
                        _ if payload["stream"].as_bool().unwrap_or(false) => {
                            axum::response::Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from(SSE_BODY))
                                .unwrap()
                        }
                        _ => {
                            // Echo the Authorization header and model we received
                            // so tests can prove the key swap and the router /
                            // budget model rewrites reached the upstream.
                            let auth = headers
                                .get("authorization")
                                .and_then(|v| v.to_str().ok())
                                .unwrap_or("");
                            let model = payload["model"].as_str().unwrap_or("");
                            let body = format!(
                                r#"{{"echoed_auth":"{auth}","model":"{model}","usage":{{"prompt_tokens":5,"completion_tokens":2}}}}"#
                            );
                            axum::response::Response::builder()
                                .header("content-type", "application/json")
                                .body(Body::from(body))
                                .unwrap()
                        }
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}/v1/chat/completions")
    }

    fn state_with_store(
        upstream_url: &str,
        tenant_store: std::sync::Arc<dyn TenantStore>,
    ) -> std::sync::Arc<AppState> {
        let (tx, _rx) = mpsc::channel::<TraceMessage>(64);
        std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: upstream_url.to_string(),
            tenant_store,
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy(
                "postgres://inferon:inferon@localhost:5432/inferon",
            )
            .unwrap(),
        })
    }

    fn test_state(upstream_url: &str) -> std::sync::Arc<AppState> {
        state_with_store(upstream_url, std::sync::Arc::new(seeded_store()))
    }

    fn failing_state(upstream_url: &str) -> std::sync::Arc<AppState> {
        state_with_store(upstream_url, std::sync::Arc::new(FailingStore))
    }

    fn chat_request_builder(body: &str, auth: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("content-type", "application/json");
        if let Some(key) = auth {
            builder = builder.header("authorization", format!("Bearer {key}"));
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    async fn chat_request(body: &str, auth: Option<&str>) -> axum::response::Response {
        build_router(test_state(&mock_upstream().await))
            .oneshot(chat_request_builder(body, auth))
            .await
            .unwrap()
    }

    async fn traced_request(
        body: &str,
        auth: Option<&str>,
    ) -> (axum::response::Response, mpsc::Receiver<TraceMessage>) {
        traced_request_with_budget(
            body,
            auth,
            std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
        )
        .await
    }

    /// `traced_request` with a caller-supplied budget store — used to pin the
    /// ITEM-027 guard opt-out (a tenant config with guard_enabled = false).
    async fn traced_request_with_budget(
        body: &str,
        auth: Option<&str>,
        budget_store: std::sync::Arc<dyn budget::BudgetStore>,
    ) -> (axum::response::Response, mpsc::Receiver<TraceMessage>) {
        let (tx, rx) = mpsc::channel(64);
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: mock_upstream().await,
            tenant_store: std::sync::Arc::new(seeded_store()),
            budget_store,
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy(
                "postgres://inferon:inferon@localhost:5432/inferon",
            )
            .unwrap(),
        });
        let response = build_router(state)
            .oneshot(chat_request_builder(body, auth))
            .await
            .unwrap();
        (response, rx)
    }

    fn dummy_trace() -> TraceMessage {
        TraceMessage {
            id: uuid::Uuid::new_v4(),
            tenant_id: uuid::Uuid::parse_str(TENANT_ID).unwrap(),
            request_model: "deepseek-chat".into(),
            system_prompt_hash: None,
            prompt_tokens: 1,
            completion_tokens: 1,
            cached_prompt_tokens: None,
            ttft_ms: Some(1),
            total_duration_ms: 1,
            status_code: 200,
            raw_prompt: None,
            raw_completion: None,
            waste_detected: None,
            upstream_cost_usd: rust_decimal::Decimal::ZERO,
            truncated: false,
            guard_event: None,
        }
    }

    #[tokio::test]
    async fn healthz_returns_200_ok() {
        let response = build_router(test_state("http://127.0.0.1:1/"))
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[test]
    fn parse_port_defaults_to_8080() {
        assert_eq!(parse_port(None), 8080);
    }

    #[test]
    fn parse_port_reads_explicit_value() {
        assert_eq!(parse_port(Some("9090")), 9090);
    }

    #[tokio::test]
    async fn ping_health_returns_true_when_server_is_up() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = build_router(test_state("http://127.0.0.1:1/"));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert!(ping_health(port).await);
    }

    #[tokio::test]
    async fn ping_health_returns_false_when_nothing_is_listening() {
        assert!(!ping_health(1).await);
    }

    #[tokio::test]
    async fn stream_request_passes_sse_through_byte_identical() {
        let response = chat_request(
            r#"{"model":"deepseek-chat","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/event-stream"
        );
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), SSE_BODY.as_bytes());
    }

    #[tokio::test]
    async fn non_stream_request_passes_json_through_with_usage() {
        let response = chat_request(
            r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/json"
        );
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["usage"]["prompt_tokens"], 5);
        assert_eq!(json["usage"]["completion_tokens"], 2);
    }

    /// State with a budget configuration for the seeded tenant.
    fn budgeted_state(
        upstream_url: &str,
        budget: budget::TenantBudget,
    ) -> std::sync::Arc<AppState> {
        let mut budgets = std::collections::HashMap::new();
        budgets.insert(uuid::Uuid::parse_str(TENANT_ID).unwrap(), budget);
        let (tx, _rx) = mpsc::channel::<TraceMessage>(64);
        std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: upstream_url.to_string(),
            tenant_store: std::sync::Arc::new(seeded_store()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::new(budgets)),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy(
                "postgres://inferon:inferon@localhost:5432/inferon",
            )
            .unwrap(),
        })
    }

    /// Same state as `state` but with a fresh telemetry channel so the caller
    /// can observe emitted traces.
    fn with_trace_channel(
        state: &std::sync::Arc<AppState>,
    ) -> (std::sync::Arc<AppState>, mpsc::Receiver<TraceMessage>) {
        let (tx, rx) = mpsc::channel(64);
        let state = std::sync::Arc::new(AppState {
            http_client: state.http_client.clone(),
            upstream_url: state.upstream_url.clone(),
            tenant_store: state.tenant_store.clone(),
            budget_store: state.budget_store.clone(),
            operator_key: state.operator_key.clone(),
            accounts: None,
            db: state.db.clone(),
            telemetry_tx: tx,
        });
        (state, rx)
    }

    async fn json_body(response: axum::response::Response) -> Value {
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn simple_gpt4o_request_is_downgraded_to_mini() {
        let (response, mut rx) = traced_request(
            r#"{"model":"gpt-4o","messages":[{"role":"user","content":"write a haiku"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let json = json_body(response).await;
        assert_eq!(json["model"], "gpt-4o-mini", "upstream must see the downgraded model");
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.request_model, "gpt-4o-mini", "trace reflects the actual model");
    }

    #[tokio::test]
    async fn tool_using_gpt4o_is_not_downgraded() {
        let (response, _rx) = traced_request(
            r#"{"model":"gpt-4o","tools":[{"type":"function"}],"messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let json = json_body(response).await;
        assert_eq!(json["model"], "gpt-4o", "complex requests keep the client model");
    }

    #[tokio::test]
    async fn deepseek_requests_are_never_rewritten() {
        let (response, mut rx) = traced_request(
            r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let json = json_body(response).await;
        assert_eq!(json["model"], "deepseek-chat");
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.request_model, "deepseek-chat");
    }

    #[tokio::test]
    async fn hard_cap_budget_rejects_with_429_and_traces() {
        let base = budgeted_state(
            &mock_upstream().await,
            budget::TenantBudget {
                limit_cents: 1000,
                spent_cents: 1000,
                cheap_fallback_model: Some("gpt-4o-mini".to_string()),
            guard_enabled: true,
            },
        );
        let (state, mut rx) = with_trace_channel(&base);
        let response = build_router(state)
            .oneshot(chat_request_builder(
                r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
                Some(CLIENT_KEY),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.status_code, 429);
    }

    #[tokio::test]
    async fn near_cap_budget_downgrades_to_cheap_model() {
        let state = budgeted_state(
            &mock_upstream().await,
            budget::TenantBudget {
                limit_cents: 1000,
                spent_cents: 900, // in the last 15% zone
                cheap_fallback_model: Some("gpt-4o-mini".to_string()),
            guard_enabled: true,
            },
        );
        let response = build_router(state)
            .oneshot(chat_request_builder(
                r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
                Some(CLIENT_KEY),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let json = json_body(response).await;
        assert_eq!(json["model"], "gpt-4o-mini", "budget fallback rewrites the model");
    }

    #[tokio::test]
    async fn within_budget_requests_forward_unchanged() {
        let state = budgeted_state(
            &mock_upstream().await,
            budget::TenantBudget {
                limit_cents: 1000,
                spent_cents: 100,
                cheap_fallback_model: Some("gpt-4o-mini".to_string()),
            guard_enabled: true,
            },
        );
        let response = build_router(state)
            .oneshot(chat_request_builder(
                r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
                Some(CLIENT_KEY),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let json = json_body(response).await;
        assert_eq!(json["model"], "deepseek-chat");
    }

    #[tokio::test]
    async fn upstream_transport_failure_returns_502() {
        let state = test_state("http://127.0.0.1:1/v1/chat/completions");
        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {CLIENT_KEY}"))
                    .body(Body::from(
                        r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::BAD_GATEWAY);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["type"], "upstream_error");
    }

    #[tokio::test]
    async fn upstream_http_error_is_forwarded_unchanged() {
        let response = chat_request(
            r#"{"model":"rate","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn missing_auth_header_returns_401() {
        let response = chat_request(
            r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
            None,
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn unknown_api_key_returns_401() {
        let response = chat_request(
            r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
            Some("sk-nope"),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn valid_key_swaps_upstream_auth_header() {
        let response = chat_request(
            r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["echoed_auth"], format!("Bearer {MASTER_KEY}"));
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            !body_str.contains(CLIENT_KEY),
            "client key must never reach the upstream"
        );
    }

    #[tokio::test]
    async fn non_json_body_returns_400() {
        let response = chat_request("this is not json", Some(CLIENT_KEY)).await;

        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    // --- ITEM-034: api keys as rows (multi-key + revoke) ---

    const SECOND_KEY: &str = "sk-client-dev-key-2";
    /// `seeded_store` plus a second active key (SECOND_KEY) for the same tenant.
    async fn store_with_second_key() -> (InMemoryTenantStore, uuid::Uuid) {
        let store = seeded_store();
        let id = store
            .insert_api_key(
                uuid::Uuid::parse_str(TENANT_ID).unwrap(),
                "ci",
                &tenant_hash(SECOND_KEY),
            )
            .await
            .expect("mint succeeds");
        (store, id)
    }

    #[tokio::test]
    async fn insert_tenant_creates_a_tenant_keys_can_attach_to() {
        // ITEM-036: self-serve provisioning mints a tenant row, then attaches
        // an owner membership and a first key through the ITEM-034 path.
        let store = InMemoryTenantStore::default();
        let upstream = store.insert_tenant(Some("sk-upstream")).await.expect("ok");
        let tenant = store.tenant_by_id(upstream).await.expect("ok").expect("tenant present");
        assert_eq!(tenant.upstream_openai_key.as_deref(), Some("sk-upstream"));

        let keyless = store.insert_tenant(None).await.expect("ok");
        assert!(store.tenant_by_id(keyless).await.expect("ok").unwrap().upstream_openai_key.is_none());

        // A minted key on the provisioned tenant resolves back to it.
        let hash = tenant_hash("sk-provisioned-dev");
        store.insert_api_key(upstream, "default", &hash).await.expect("key minted");
        let resolved = store.tenant_by_api_key_hash(&hash).await.expect("ok");
        assert_eq!(resolved.map(|t| t.id), Some(upstream));
    }

    #[tokio::test]
    async fn revoke_tenant_key_only_touches_the_owners_keys() {
        // ITEM-038 AC2: a tenant-scoped revoke can never kill another tenant's
        // key (tenant boundaries hold even with a known key id).
        let store = InMemoryTenantStore::default();
        let tenant_a = store.insert_tenant(Some("a")).await.expect("ok");
        let tenant_b = store.insert_tenant(Some("b")).await.expect("ok");
        let key_a = store
            .insert_api_key(tenant_a, "a-key", "hash-a")
            .await
            .expect("ok");
        let key_b = store
            .insert_api_key(tenant_b, "b-key", "hash-b")
            .await
            .expect("ok");
        // Tenant A cannot revoke B's key…
        assert!(!store.revoke_tenant_key(tenant_a, key_b).await.expect("ok"));
        // …and B's key still authenticates.
        assert!(store.tenant_by_api_key_hash("hash-b").await.expect("ok").is_some());
        // Tenant A revokes its own key; revoking again is a no-op.
        assert!(store.revoke_tenant_key(tenant_a, key_a).await.expect("ok"));
        assert!(!store.revoke_tenant_key(tenant_a, key_a).await.expect("ok"));
        assert!(store.tenant_by_api_key_hash("hash-a").await.expect("ok").is_none());
    }

    #[tokio::test]
    async fn a_second_active_key_authenticates_to_the_same_tenant() {
        let (store, _) = store_with_second_key().await;
        let tenant = store
            .tenant_by_api_key_hash(&tenant_hash(SECOND_KEY))
            .await
            .expect("store ok")
            .expect("second key resolves");
        assert_eq!(tenant.id.to_string(), TENANT_ID);
        // Both keys share the tenant's upstream key.
        assert_eq!(tenant.upstream_openai_key.as_deref(), Some(MASTER_KEY));
    }

    #[tokio::test]
    async fn revoking_a_key_stops_it_authenticating_immediately() {        let (store, second_key_id) = store_with_second_key().await;
        assert!(store
            .revoke_api_key(second_key_id)
            .await
            .expect("revoke ok"), "revoked a live key");
        assert_eq!(
            store
                .tenant_by_api_key_hash(&tenant_hash(SECOND_KEY))
                .await
                .expect("store ok"),
            None,
            "revoked key resolves to nothing"
        );
        // The original key is untouched.
        assert!(store
            .tenant_by_api_key_hash(&tenant_hash(CLIENT_KEY))
            .await
            .expect("store ok")
            .is_some());
        // Revoking again (or an unknown id) reports no-op.
        assert!(!store
            .revoke_api_key(second_key_id)
            .await
            .expect("revoke ok"), "already revoked");
        assert!(!store
            .revoke_api_key(uuid::Uuid::new_v4())
            .await
            .expect("revoke ok"), "unknown key id");
    }

    #[tokio::test]
    async fn key_mint_rejects_unknown_tenants_and_list_reports_revocation() {
        let store = seeded_store();
        assert!(store
            .insert_api_key(uuid::Uuid::new_v4(), "ci", &tenant_hash("sk-x"))
            .await
            .is_err(), "minting for a missing tenant fails");
        let (store, second_key_id) = store_with_second_key().await;
        let tenant_id = uuid::Uuid::parse_str(TENANT_ID).unwrap();
        let listed = store.list_api_keys(tenant_id).await.expect("list ok");
        assert_eq!(listed.len(), 2, "seeded + minted key");
        assert!(listed.iter().any(|k| k.label == "ci" && !k.revoked()));
        store.revoke_api_key(second_key_id).await.expect("revoke ok");
        let listed = store.list_api_keys(tenant_id).await.expect("list ok");
        assert!(listed.iter().any(|k| k.label == "ci" && k.revoked()));
        assert_eq!(
            store.list_api_keys(uuid::Uuid::new_v4()).await.expect("list ok").len(),
            0,
            "no keys for other tenants"
        );
    }

    /// Proxy-level check: chat request against a custom store + custom key.
    async fn chat_request_with_store(body: &str, key: &str, store: InMemoryTenantStore) -> axum::response::Response {
        let state = state_with_store(
            &mock_upstream().await,
            std::sync::Arc::new(store),
        );
        build_router(state)
            .oneshot(chat_request_builder(body, Some(key)))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn revoked_key_returns_401_while_other_keys_keep_working() {
        let body = r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#;
        // Second active key works end-to-end.
        let (store, _) = store_with_second_key().await;
        let ok = chat_request_with_store(body, SECOND_KEY, store).await;
        assert_eq!(ok.status(), axum::http::StatusCode::OK);

        // After revocation the same key is 401; the original key still works.
        let (store, second_key_id) = store_with_second_key().await;
        store.revoke_api_key(second_key_id).await.expect("revoke ok");
        let denied = chat_request_with_store(body, SECOND_KEY, store.clone()).await;
        assert_eq!(denied.status(), axum::http::StatusCode::UNAUTHORIZED);
        let ok2 = chat_request_with_store(body, CLIENT_KEY, store).await;
        assert_eq!(ok2.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn tenant_store_failure_returns_500() {
        let state = failing_state("http://127.0.0.1:1/v1/chat/completions");
        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {CLIENT_KEY}"))
                    .body(Body::from(
                        r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    // --- ITEM-005 telemetry ---

    #[test]
    fn usage_from_sse_bytes_parses_final_usage_frame() {
        assert_eq!(
            usage_from_sse_bytes(SSE_BODY.as_bytes()),
            Some(Usage {
                prompt_tokens: 5,
                completion_tokens: 2,
                cached_tokens: None,
            })
        );
    }

    #[test]
    fn usage_from_sse_bytes_handles_frames_split_across_chunks() {
        let mut acc = Vec::new();
        for chunk in SSE_BODY.as_bytes().chunks(7) {
            acc.extend_from_slice(chunk);
        }
        assert_eq!(
            usage_from_sse_bytes(&acc),
            Some(Usage {
                prompt_tokens: 5,
                completion_tokens: 2,
                cached_tokens: None,
            })
        );
    }

    #[test]
    fn usage_from_sse_bytes_returns_none_when_absent() {
        assert_eq!(usage_from_sse_bytes(b"data: {\"a\":1}\n\ndata: [DONE]\n\n"), None);
        assert_eq!(usage_from_sse_bytes(b""), None);
        assert_eq!(usage_from_sse_bytes(b"not sse at all"), None);
    }

    #[test]
    fn usage_from_sse_bytes_reads_cache_evidence() {
        let sse = "data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2,\"prompt_tokens_details\":{\"cached_tokens\":7}}}\n\n";
        assert_eq!(
            usage_from_sse_bytes(sse.as_bytes()),
            Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 2,
                cached_tokens: Some(7),
            })
        );
    }

    #[test]
    fn system_prompt_hash_hashes_first_system_message() {
        let payload: serde_json::Value = serde_json::json!({
            "messages": [
                {"role": "system", "content": "be terse"},
                {"role": "system", "content": "second"},
                {"role": "user", "content": "hi"}
            ]
        });
        assert_eq!(
            system_prompt_hash(&payload).as_deref(),
            Some(tenant_hash("be terse").as_str())
        );
    }

    #[test]
    fn system_prompt_hash_is_none_without_system_message() {
        let payload: serde_json::Value =
            serde_json::json!({"messages": [{"role": "user", "content": "hi"}]});
        assert_eq!(system_prompt_hash(&payload), None);
        assert_eq!(system_prompt_hash(&serde_json::json!({})), None);
    }

    #[tokio::test]
    async fn non_stream_request_emits_trace_with_tokens_and_raw() {
        let (response, mut rx) = traced_request(
            r#"{"model":"deepseek-chat","messages":[{"role":"system","content":"be terse"},{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let msg = rx.recv().await.expect("trace should be emitted");
        assert_eq!(msg.tenant_id, uuid::Uuid::parse_str(TENANT_ID).unwrap());
        assert_eq!(msg.request_model, "deepseek-chat");
        assert_eq!(msg.prompt_tokens, 5);
        assert_eq!(msg.completion_tokens, 2);
        assert_eq!(msg.status_code, 200);
        assert_eq!(
            msg.system_prompt_hash.as_deref(),
            Some(tenant_hash("be terse").as_str())
        );
        assert_eq!(
            msg.raw_prompt.as_ref().unwrap()["messages"][0]["role"],
            "system"
        );
        assert_eq!(
            msg.raw_completion.as_ref().unwrap()["usage"]["prompt_tokens"],
            5
        );
    }

    #[tokio::test]
    async fn non_stream_without_system_message_has_null_hash_and_ttft() {
        let (response, mut rx) = traced_request(
            r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let msg = rx.recv().await.expect("trace should be emitted");
        assert_eq!(msg.system_prompt_hash, None);
        assert!(msg.ttft_ms.is_some(), "non-stream ttft mirrors total duration");
        assert!(msg.total_duration_ms >= 0);
        assert_eq!(msg.prompt_tokens, 5);
    }

    #[tokio::test]
    async fn stream_request_emits_trace_with_usage_from_final_chunk() {
        let (response, mut rx) = traced_request(
            r#"{"model":"deepseek-chat","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), SSE_BODY.as_bytes());
        let msg = rx
            .recv()
            .await
            .expect("trace emitted after the stream body is consumed");
        assert_eq!(msg.prompt_tokens, 5, "tokens parsed from final chunk");
        assert_eq!(msg.completion_tokens, 2);
        assert!(msg.ttft_ms.is_some(), "ttft recorded at first chunk");
        assert!(msg.total_duration_ms >= 0);
        assert_eq!(msg.status_code, 200);
        assert_eq!(msg.raw_completion, None, "stream raw deferred (per spec)");
    }

    #[tokio::test]
    async fn stream_without_usage_frame_records_zero_tokens() {
        let (response, mut rx) = traced_request(
            r#"{"model":"nousage","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let _body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let msg = rx.recv().await.expect("trace should be emitted");
        assert_eq!(msg.prompt_tokens, 0);
        assert_eq!(msg.completion_tokens, 0);
    }

    #[tokio::test]
    async fn upstream_error_response_is_traced_with_status() {
        let (response, mut rx) = traced_request(
            r#"{"model":"rate","messages":[{"role":"user","content":"hi"}]}"#,
            Some(CLIENT_KEY),
        )
        .await;

        assert_eq!(response.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        let _body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let msg = rx.recv().await.expect("trace should be emitted");
        assert_eq!(msg.status_code, 429);
        assert_eq!(msg.prompt_tokens, 0);
    }

    #[tokio::test]
    async fn telemetry_send_never_blocks_when_channel_full() {
        let (tx, rx_keepalive) = mpsc::channel::<TraceMessage>(1);
        // Keep the receiver alive but never drain it: the channel fills after
        // the first request's trace. Later requests must still complete (AC5).
        let _keepalive = rx_keepalive;
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: mock_upstream().await,
            tenant_store: std::sync::Arc::new(seeded_store()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy(
                "postgres://inferon:inferon@localhost:5432/inferon",
            )
            .unwrap(),
        });

        let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            for _ in 0..3 {
                let response = build_router(state.clone())
                    .oneshot(chat_request_builder(
                        r#"{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}]}"#,
                        Some(CLIENT_KEY),
                    ))
                    .await
                    .unwrap();
                assert_eq!(response.status(), axum::http::StatusCode::OK);
                let _ = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap();
            }
        })
        .await;
        assert!(
            result.is_ok(),
            "requests must not block on a full telemetry channel"
        );
    }

    // Worker batching policy — sink-agnostic (the Postgres sink is exercised at
    // the deployment seam where a real database exists).
    #[derive(Default)]
    struct SinkCounters {
        flushes: usize,
        messages: usize,
        cache_miss_flags: usize,
        over_provisioned_flags: usize,
    }

    struct SharedSink(std::sync::Arc<tokio::sync::Mutex<SinkCounters>>);

    #[async_trait::async_trait]
    impl TraceSink for SharedSink {
        async fn flush(&mut self, batch: &mut Vec<TraceMessage>) {
            let mut counters = self.0.lock().await;
            counters.flushes += 1;
            for msg in batch.drain(..) {
                counters.messages += 1;
                if let Some(waste) = &msg.waste_detected {
                    if waste["cache_miss_system_prompt"] == true {
                        counters.cache_miss_flags += 1;
                    }
                    if waste["over_provisioned"] == true {
                        counters.over_provisioned_flags += 1;
                    }
                }
            }
        }
    }

    fn spawn_worker(
        rx: mpsc::Receiver<TraceMessage>,
        flush_interval_ms: u64,
        batch_size: usize,
    ) -> std::sync::Arc<tokio::sync::Mutex<SinkCounters>> {
        spawn_worker_with_counts(rx, flush_interval_ms, batch_size, HashMap::new())
    }

    fn spawn_worker_with_counts(
        rx: mpsc::Receiver<TraceMessage>,
        flush_interval_ms: u64,
        batch_size: usize,
        initial_counts: HashMap<String, u32>,
    ) -> std::sync::Arc<tokio::sync::Mutex<SinkCounters>> {
        let counters = std::sync::Arc::new(tokio::sync::Mutex::new(SinkCounters::default()));
        let sink: Box<dyn TraceSink> = Box::new(SharedSink(counters.clone()));
        tokio::spawn(telemetry_worker(
            rx,
            sink,
            std::time::Duration::from_millis(flush_interval_ms),
            batch_size,
            initial_counts,
        ));
        counters
    }

    async fn wait_until(
        counters: std::sync::Arc<tokio::sync::Mutex<SinkCounters>>,
        predicate: impl Fn(&SinkCounters) -> bool,
    ) -> bool {
        for _ in 0..200 {
            if predicate(&*counters.lock().await) {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        false
    }

    #[tokio::test]
    async fn worker_flushes_when_batch_threshold_reached() {
        let (tx, rx) = mpsc::channel(256);
        let counters = spawn_worker(rx, 60_000, 5); // interval never fires in this test
        for _ in 0..5 {
            tx.send(dummy_trace()).await.unwrap();
        }
        assert!(
            wait_until(counters.clone(), |c| c.messages >= 5).await,
            "worker should flush once 5 messages accumulate"
        );
        let counters = counters.lock().await;
        assert_eq!(counters.messages, 5);
        assert_eq!(counters.flushes, 1, "one transactional flush per batch");
    }

    #[tokio::test]
    async fn worker_honors_counts_persisted_before_restart() {
        // A restart reloads counts from the DB (initial_counts). A prompt seen
        // once before the restart must flag on its first post-restart hit.
        let (tx, rx) = mpsc::channel(256);
        let mut persisted = HashMap::new();
        persisted.insert("hash-repeated".to_string(), 1);
        let counters = spawn_worker_with_counts(rx, 60_000, 100, persisted);
        let mut msg = dummy_trace();
        msg.system_prompt_hash = Some("hash-repeated".to_string());
        msg.prompt_tokens = 5000;
        msg.cached_prompt_tokens = Some(0);
        tx.send(msg).await.unwrap();
        drop(tx);
        assert!(
            wait_until(counters.clone(), |c| c.cache_miss_flags >= 1).await,
            "second-ever occurrence must be flagged even across a restart"
        );
    }

    #[tokio::test]
    async fn worker_flushes_on_interval_when_idle() {
        let (tx, rx) = mpsc::channel(256);
        let counters = spawn_worker(rx, 40, 100);
        tx.send(dummy_trace()).await.unwrap();
        assert!(
            wait_until(counters.clone(), |c| c.messages >= 1).await,
            "worker should flush on the interval even below the batch size"
        );
        drop(tx);
    }

    #[tokio::test]
    async fn worker_flushes_remainder_and_exits_when_senders_drop() {
        let (tx, rx) = mpsc::channel(256);
        let counters = spawn_worker(rx, 60_000, 100);
        tx.send(dummy_trace()).await.unwrap();
        tx.send(dummy_trace()).await.unwrap();
        drop(tx); // all senders gone -> worker drains the remainder and exits
        assert!(
            wait_until(counters.clone(), |c| c.messages >= 2).await,
            "worker should flush the remainder on channel close"
        );
    }

    /// Drive a real HTTP request through axum::serve and close the socket
    /// after the first SSE frame — a genuine client abort mid-stream.
    async fn serve_and_abort(model: &str) -> mpsc::Receiver<TraceMessage> {
        let (tx, rx) = mpsc::channel(8);
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: mock_upstream().await,
            tenant_store: std::sync::Arc::new(seeded_store()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy(
                "postgres://inferon:inferon@localhost:5432/inferon",
            )
            .unwrap(),
        });
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, build_router(state)).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let body = format!(
            r#"{{"model":"{model}","stream":true,"messages":[{{"role":"user","content":"hi"}}]}}"#
        );
        let request = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAuthorization: Bearer {CLIENT_KEY}\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        socket.write_all(request.as_bytes()).await.unwrap();
        // Read until at least the first SSE frame has arrived, then hang for a
        // moment inside the mock's 300ms gap and drop the connection.
        let mut buf = [0u8; 2048];
        let mut seen = 0usize;
        while seen < buf.len() {
            let n = socket.read(&mut buf[seen..]).await.unwrap();
            if n == 0 {
                break;
            }
            seen += n;
            if String::from_utf8_lossy(&buf[..seen]).contains("content\":\"a\"") {
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        drop(socket); // abort
        rx
    }

    #[tokio::test]
    async fn client_abort_mid_stream_records_single_truncated_trace() {
        let mut rx = serve_and_abort("slow").await;
        let msg = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
            .await
            .expect("truncated trace should be emitted")
            .expect("channel open");
        assert!(msg.truncated, "aborted stream must be flagged truncated");
        assert_eq!(msg.status_code, 200);
        assert!(msg.raw_completion.is_none());
        // Exactly one trace: the abort must not also fire the normal emit.
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        assert!(rx.try_recv().is_err(), "no duplicate trace after abort");
    }

    #[tokio::test]
    async fn upstream_mid_stream_error_records_truncated_trace() {
        let (tx, mut rx) = mpsc::channel(8);
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: mock_upstream().await,
            tenant_store: std::sync::Arc::new(seeded_store()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy(
                "postgres://inferon:inferon@localhost:5432/inferon",
            )
            .unwrap(),
        });
        let response = build_router(state)
            .oneshot(chat_request_builder(
                r#"{"model":"miderror","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
                Some(CLIENT_KEY),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&body).contains("content\":\"a\""),
            "client still receives the frames before the failure"
        );
        let msg = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
            .await
            .expect("trace should be emitted")
            .expect("channel open");
        assert!(msg.truncated, "mid-stream upstream error must be flagged truncated");
    }

    #[tokio::test]
    async fn worker_lints_repeated_uncached_system_prompts_off_the_hot_path() {
        let (tx, rx) = mpsc::channel(256);
        let counters = spawn_worker(rx, 60_000, 100);
        // Two identical traces: same hash, >=4000 prompt tokens, no cache benefit.
        for _ in 0..2 {
            let mut msg = dummy_trace();
            msg.system_prompt_hash = Some("hash-repeated".to_string());
            msg.prompt_tokens = 5000;
            msg.cached_prompt_tokens = Some(0);
            tx.send(msg).await.unwrap();
        }
        drop(tx);
        assert!(
            wait_until(counters.clone(), |c| c.cache_miss_flags >= 1).await,
            "second occurrence of a large uncached system prompt must be flagged"
        );
    }

    // --- ITEM-023: schema guard buffered validation seam ---

    /// A streamed request carrying a JSON-schema contract and a cheap-tier
    /// model (gpt-4o-mini) — the guard's trigger condition.
    fn guarded_stream_body(model: &str, marker: &str) -> String {
        serde_json::json!({
            "model": model,
            "stream": true,
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "answer",
                    "schema": {"type": "object", "properties": {"answer": {"type": "string"}}, "required": ["answer"]}
                }
            },
            "messages": [{"role": "user", "content": marker}]
        })
        .to_string()
    }

    #[tokio::test]
    async fn guarded_valid_stream_buffers_then_flushes_after_done() {
        // gpt-4o-mini + schema contract + stream -> guarded path. The mock's
        // generic streaming route returns a well-formed SSE body; the client
        // must receive it whole (buffered, then flushed after [DONE]).
        let (response, mut rx) =
            traced_request(&guarded_stream_body("gpt-4o-mini", "valid-json"), Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/event-stream"
        );
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let expected = delta_only_stream(r#"{"answer":"42"}"#);
        assert_eq!(
            body.as_ref(),
            expected.as_bytes(),
            "buffered schema-valid body flushed whole after [DONE]"
        );
        let msg = rx.recv().await.expect("trace emitted");
        assert_eq!(msg.status_code, 200);
        assert!(!msg.truncated, "clean guarded flush is not truncated");
        // 026 AC1: every guard event records outcome on the trace — a clean
        // cheap flush is outcome "flushed", no failure reason.
        let ev = msg.guard_event.as_ref().expect("guard event recorded on flush");
        assert_eq!(ev["outcome"], "flushed");
        assert_eq!(ev["guarded_model"], "gpt-4o-mini");
        assert!(ev.get("reason").is_none(), "no failure reason on a clean flush");
    }

    #[tokio::test]
    async fn guarded_schema_violation_falls_back_to_the_frontier() {
        // 025 AC1: the cheap model streams well-formed JSON that violates the
        // answer schema ("wrong-shape" -> {"other":1}). The bad buffer is
        // discarded and the original prompt replayed on the tenant's frontier
        // (deepseek-v4-pro — a cheap original escalates to the deployment
        // frontier). The client receives one valid 200 SSE response and one
        // trace carrying the guard event (AC2/AC3).
        let (response, mut rx) =
            traced_request(&guarded_stream_body("gpt-4o-mini", "wrong-shape"), Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/event-stream"
        );
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let expected = delta_only_stream(r#"{"answer":"42"}"#);
        assert_eq!(
            body.as_ref(),
            expected.as_bytes(),
            "frontier replay flushed whole — never a cheap partial body"
        );
        let msg = rx.recv().await.expect("exactly one trace");
        assert_eq!(msg.status_code, 200);
        assert!(!msg.truncated, "clean fallback is not truncated");
        assert_eq!(
            msg.request_model, "deepseek-v4-pro",
            "trace model = the model that ultimately served (cost basis)"
        );
        let ev = msg.guard_event.as_ref().expect("guard event recorded");
        assert_eq!(ev["outcome"], "fallback");
        assert_eq!(ev["reason"], "schema_violation");
        assert_eq!(ev["guarded_model"], "gpt-4o-mini");
        assert!(rx.try_recv().is_err(), "fallback is not a second request");
    }

    #[tokio::test]
    async fn guarded_truncated_upstream_falls_back_to_the_frontier() {
        // Container failure (JSON frames but no [DONE]) is a classified
        // UpstreamFailure: the buffer is discarded and the request falls back
        // instead of showing the client a truncated cheap body.
        let (response, mut rx) =
            traced_request(&guarded_stream_body("gpt-4o-mini", "truncated"), Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let expected = delta_only_stream(r#"{"answer":"42"}"#);
        assert_eq!(body.as_ref(), expected.as_bytes());
        let msg = rx.recv().await.expect("exactly one trace");
        assert!(!msg.truncated);
        let ev = msg.guard_event.as_ref().expect("guard event recorded");
        assert_eq!(ev["outcome"], "fallback");
        assert_eq!(ev["reason"], "upstream_failure");
        assert_eq!(msg.request_model, "deepseek-v4-pro");
    }

    #[tokio::test]
    async fn downgraded_frontier_request_falls_back_to_the_original_model() {
        // The tenant asked gpt-4o; the router downgraded it to gpt-4o-mini and
        // the cheap response violated the schema. The replay must target the
        // ORIGINALLY REQUESTED model (gpt-4o), not the deployment default.
        let (response, mut rx) =
            traced_request(&guarded_stream_body("gpt-4o", "wrong-shape"), Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let expected = delta_only_stream(r#"{"answer":"42"}"#);
        assert_eq!(body.as_ref(), expected.as_bytes());
        let msg = rx.recv().await.expect("exactly one trace");
        assert_eq!(msg.request_model, "gpt-4o", "replayed to the original model");
        let ev = msg.guard_event.as_ref().expect("guard event recorded");
        assert_eq!(ev["guarded_model"], "gpt-4o-mini", "the guard fired on the downgrade");
        assert_eq!(ev["reason"], "schema_violation");
    }

    #[tokio::test]
    async fn guarded_fallback_upstream_failure_is_loud_and_clean() {
        // 025 AC4: the frontier replay itself fails (mock 500 for model
        // deepseek-v4-pro on marker "boom"). The client gets one clean 502 —
        // no silent data loss — and the single trace preserves the guard
        // reason plus the fallback failure detail.
        let (response, mut rx) =
            traced_request(&guarded_stream_body("gpt-4o-mini", "boom"), Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), axum::http::StatusCode::BAD_GATEWAY);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["type"], "guard_fallback_error");
        let msg = rx.recv().await.expect("exactly one trace");
        assert_eq!(msg.status_code, 200, "answered (not truncated)");
        assert_eq!(msg.request_model, "deepseek-v4-pro");
        let ev = msg.guard_event.as_ref().expect("guard event recorded");
        assert_eq!(ev["outcome"], "fallback_failed");
        assert_eq!(ev["reason"], "upstream_failure", "the original guard reason");
        assert_eq!(ev["detail"], "http 500");
        assert!(rx.try_recv().is_err(), "still exactly one trace");
    }

    #[tokio::test]
    async fn tenant_guard_opt_out_restores_byte_for_byte_passthrough() {
        // 027 AC1: with tenant_configurations.guard_enabled = false the lying
        // cheap stream pipes through unchanged (no buffering, no fallback) and
        // no guard event is recorded — the opt-out is per tenant, everyone
        // else keeps the default-on guard.
        let tenant_id = uuid::Uuid::parse_str(TENANT_ID).unwrap();
        let mut budgets = std::collections::HashMap::new();
        budgets.insert(
            tenant_id,
            budget::TenantBudget {
                limit_cents: 0,
                spent_cents: 0,
                cheap_fallback_model: None,
                guard_enabled: false,
            },
        );
        let store: std::sync::Arc<dyn budget::BudgetStore> =
            std::sync::Arc::new(budget::InMemoryBudgetStore::new(budgets));
        let (response, mut rx) = traced_request_with_budget(
            &guarded_stream_body("gpt-4o-mini", "wrong-shape"),
            Some(CLIENT_KEY),
            store,
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let expected = delta_only_stream(r#"{"other":1}"#);
        assert_eq!(
            body.as_ref(),
            expected.as_bytes(),
            "opt-out tenant gets the raw cheap body, no fallback"
        );
        let msg = rx.recv().await.expect("trace emitted");
        assert_eq!(msg.request_model, "gpt-4o-mini");
        assert!(
            msg.guard_event.is_none(),
            "opt-out tenant records no guard event"
        );
    }

    #[tokio::test]
    async fn unguarded_stream_stays_byte_for_byte_unchanged() {
        // Same cheap model but NO schema contract: pure passthrough, even for
        // a body the guard would reject (IT-023 AC2: unguarded = zero change).
        let body = r#"{"model":"nodone","stream":true,"messages":[{"role":"user","content":"hi"}]}"#;
        let (response, _rx) = traced_request(body, Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let out = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(
            text.contains("data: {") && !text.contains("[DONE]"),
            "unguarded traffic pipes the truncated upstream body straight through"
        );
    }

    #[tokio::test]
    async fn frontier_model_with_schema_contract_is_not_guarded() {
        // Same schema contract but a frontier model: not guarded, passthrough.
        let (response, mut rx) = traced_request(&guarded_stream_body("deepseek-v4-pro", "hi"), Some(CLIENT_KEY)).await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let out = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(out.as_ref(), SSE_BODY.as_bytes());
        let msg = rx.recv().await.expect("trace emitted");
        // 026 AC4: guard telemetry is additive — unguarded traces stay None.
        assert!(msg.guard_event.is_none(), "unguarded trace never gains a guard event");
    }

    /// Drive a real guarded request through axum::serve and drop the socket
    /// while the handler is still buffering (abort mid-buffer). `marker`
    /// selects the mock upstream behaviour for the cheap model: "guarded-slow"
    /// = slow cheap stream (abort before the guard verdict); "slow-fallback" =
    /// instant cheap violation then a slow frontier replay (abort mid-replay).
    async fn serve_guarded_and_abort(model: &str, marker: &str) -> mpsc::Receiver<TraceMessage> {
        let (tx, rx) = mpsc::channel(8);
        let state = std::sync::Arc::new(AppState {
            http_client: reqwest::Client::new(),
            upstream_url: mock_upstream().await,
            tenant_store: std::sync::Arc::new(seeded_store()),
            budget_store: std::sync::Arc::new(budget::InMemoryBudgetStore::default()),
            telemetry_tx: tx,
            operator_key: None,
            accounts: None,
            db: sqlx::PgPool::connect_lazy(
                "postgres://inferon:inferon@localhost:5432/inferon",
            )
            .unwrap(),
        });
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, build_router(state)).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let body = guarded_stream_body(model, marker);
        let request = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAuthorization: Bearer {CLIENT_KEY}\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        socket.write_all(request.as_bytes()).await.unwrap();
        // The guard buffers everything, so the client sees no body bytes yet —
        // the abort happens while the handler still waits upstream (300ms gap
        // between frames on the mock). Drop mid-buffer.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        drop(socket); // abort
        rx
    }

    #[tokio::test]
    async fn client_abort_during_guarded_buffer_records_truncated_trace() {
        let mut rx = serve_guarded_and_abort("gpt-4o-mini", "guarded-slow").await;
        let msg = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
            .await
            .expect("truncated trace should be emitted")
            .expect("channel open");
        assert!(msg.truncated, "abort while buffering must be flagged truncated");
        // Exactly one trace — no duplicate from a later flush.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert!(rx.try_recv().is_err(), "no duplicate trace after abort");
    }

    #[tokio::test]
    async fn client_abort_during_frontier_replay_records_one_guard_trace() {
        // 025: cheap response violated instantly, so the replay to the frontier
        // (deepseek-v4-pro, slow) is in flight when the client aborts mid-
        // replay. Exactly one trace: truncated, retargeted to the frontier,
        // still carrying the guard event.
        let mut rx = serve_guarded_and_abort("gpt-4o-mini", "slow-fallback").await;
        let msg = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
            .await
            .expect("truncated trace should be emitted")
            .expect("channel open");
        assert!(msg.truncated, "abort during the replay is flagged truncated");
        assert_eq!(msg.request_model, "deepseek-v4-pro");
        let ev = msg.guard_event.as_ref().expect("guard event recorded");
        assert_eq!(ev["outcome"], "fallback");
        assert_eq!(ev["reason"], "schema_violation");
        assert_eq!(ev["guarded_model"], "gpt-4o-mini");
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert!(rx.try_recv().is_err(), "no duplicate trace after abort");
    }
}
