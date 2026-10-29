//! Provider provisioning (ITEM-043): a tenant's own upstream credentials.
//!
//! The `ProviderStore` seam sits between the proxy/dashboard and Postgres.
//! `PostgresProviderStore` encrypts keys at rest ([`crate::secret::Secrets`]) and
//! only ever exposes a masked `key_hint` on reads; `resolve` returns the
//! decrypted credential for the request path. `InMemoryProviderStore` is the
//! test double.

use crate::secret::Secrets;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub use crate::StoreResult;

/// Optional, non-secret provider configuration (ITEM-043 AC1): an OpenAI
/// organization id and arbitrary upstream headers (e.g. `anthropic-version`
/// for a compatible gateway). Never contains the API key.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

/// A provider as shown to the dashboard — never carries key material.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provider {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub kind: String,
    pub name: String,
    pub base_url: String,
    /// Masked hint for display, e.g. `…ab12`.
    pub key_hint: String,
    pub metadata: ProviderMetadata,
    /// ITEM-047: monthly spend cap in cents; 0 = no limit.
    pub spend_limit_cents: i64,
    /// The tenant's default provider — the one the proxy uses for a model no
    /// provider kind owns. Exactly one per tenant (when any exist).
    pub is_default: bool,
    /// ITEM-051: the last Test-connection result (None = never checked).
    pub last_check_at: Option<String>,
    pub last_check_ok: Option<bool>,
    pub created_at: String,
    pub updated_at: String,
}

/// A provider to register (`key` is the raw upstream credential).
#[derive(Clone, Debug)]
pub struct NewProvider {
    pub kind: String,
    pub name: String,
    pub base_url: String,
    pub key: String,
    pub metadata: ProviderMetadata,
    pub spend_limit_cents: i64,
}

/// A partial provider update (ITEM-043 AC5 rotation). `None` leaves a field
/// unchanged; a `Some(key)` re-encrypts and re-hints.
#[derive(Clone, Debug, Default)]
pub struct ProviderUpdate {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub key: Option<String>,
    pub metadata: Option<ProviderMetadata>,
    /// ITEM-047: `Some(cents)` sets the monthly cap (0 clears it).
    pub spend_limit_cents: Option<i64>,
}

/// The decrypted credential the proxy uses for a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedProvider {
    /// The provider row id (ITEM-047: attributed on the trace).
    pub id: Uuid,
    /// The provider kind — decides the upstream surface (ITEM-050).
    pub kind: String,
    pub base_url: String,
    pub api_key: String,
    pub metadata: ProviderMetadata,
}

/// Supported provider kinds. `anthropic` is served on its own Messages surface
/// (`/v1/messages`, ITEM-050); the others are OpenAI-shaped
/// (`/v1/chat/completions`).
pub const PROVIDER_KINDS: [&str; 5] = [
    "deepseek",
    "openai",
    "openai-compatible",
    "anthropic",
    "local",
];

/// The native Anthropic Messages provider kind (ITEM-050).
pub const ANTHROPIC: &str = "anthropic";

/// A locally hosted provider (ITEM-077): inference costs nothing at the margin,
/// so its traces are costed at zero rather than the unknown-model default.
pub const LOCAL: &str = "local";

pub fn is_local(kind: &str) -> bool {
    kind == LOCAL
}

/// Whether a provider of `kind` needs a credential. A local OpenAI-compatible
/// server (Ollama, LM Studio, vLLM) usually has none, so `local` is the one kind
/// that may be attached with a base URL only (ITEM-082).
pub fn requires_key(kind: &str) -> bool {
    !is_local(kind)
}

/// True for the Anthropic Messages surface (not OpenAI-shaped).
pub fn is_anthropic(kind: &str) -> bool {
    kind == ANTHROPIC
}

/// Whether a provider of `kind` serving `model` can honour a `json_schema`
/// contract. DeepSeek only supports `json_object`; an unknown compatible
/// gateway is assumed to support it, so a request is never downgraded when it
/// might be fine.
pub fn supports_json_schema(kind: &str, model: &str) -> bool {
    !(kind == "deepseek" || model.starts_with("deepseek"))
}

pub fn is_supported_kind(kind: &str) -> bool {
    PROVIDER_KINDS.contains(&kind)
}

/// A human name when the tenant does not supply one.
pub fn default_name(kind: &str) -> String {
    match kind {
        "deepseek" => "DeepSeek".to_string(),
        "openai" => "OpenAI".to_string(),
        ANTHROPIC => "Anthropic".to_string(),
        _ => "Custom provider".to_string(),
    }
}

/// The auth header(s) for an upstream provider credential. Anthropic uses
/// `x-api-key` + `anthropic-version` (unless the tenant set its own version
/// header); every other kind uses a bearer token.
pub fn auth_headers(
    kind: &str,
    key: &str,
    metadata: &ProviderMetadata,
) -> Vec<(&'static str, String)> {
    if is_local(kind) && key.is_empty() {
        return Vec::new();
    }
    if is_anthropic(kind) {
        let mut headers = vec![("x-api-key", key.to_string())];
        let has_version = metadata
            .headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("anthropic-version"));
        if !has_version {
            headers.push(("anthropic-version", "2023-06-01".to_string()));
        }
        headers
    } else {
        vec![("Authorization", format!("Bearer {key}"))]
    }
}

/// Model ids from an OpenAI-style `{data:[{id}]}`, a `{models:[{id|name}]}`, or a
/// bare array — sorted and deduped.
pub(crate) fn parse_model_ids(body: &serde_json::Value) -> Vec<String> {
    let array = body
        .get("data")
        .and_then(serde_json::Value::as_array)
        .or_else(|| body.get("models").and_then(serde_json::Value::as_array))
        .or_else(|| body.as_array());
    let Some(array) = array else {
        return Vec::new();
    };
    let mut ids: Vec<String> = array
        .iter()
        .filter_map(|entry| {
            entry
                .get("id")
                .and_then(serde_json::Value::as_str)
                .or_else(|| entry.get("name").and_then(serde_json::Value::as_str))
                .or_else(|| entry.as_str())
                .map(str::to_string)
        })
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Read a provider's advertised models from `GET <base>/models` (the
/// OpenAI-compatible list). Shared by the dashboard probe and the edge node, so
/// an on-prem node discovers the models its local server actually serves rather
/// than the cloud's catalog (ITEM-070 follow-up).
pub(crate) async fn fetch_models(
    client: &reqwest::Client,
    provider: &ResolvedProvider,
) -> Result<Vec<String>, String> {
    let url = format!("{}/models", provider.base_url.trim_end_matches('/'));
    let mut request = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10));
    for (header, value) in auth_headers(&provider.kind, &provider.api_key, &provider.metadata) {
        request = request.header(header, value);
    }
    if let Some(org) = &provider.metadata.org {
        request = request.header("OpenAI-Organization", org);
    }
    for (header, value) in &provider.metadata.headers {
        request = request.header(header, value);
    }
    let response = request.send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("upstream returned {}", response.status()));
    }
    let body: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    Ok(parse_model_ids(&body))
}

/// `…` + the last 4 characters of a key, for display — never the whole key.
pub fn key_hint(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    let tail: String = chars[chars.len().saturating_sub(4)..].iter().collect();
    format!("\u{2026}{tail}")
}

#[async_trait::async_trait]
pub trait ProviderStore: Send + Sync {
    async fn list(&self, tenant_id: Uuid) -> StoreResult<Vec<Provider>>;
    async fn insert(&self, tenant_id: Uuid, new: &NewProvider) -> StoreResult<Uuid>;
    /// Apply a partial update in place (rotation, AC5); `false` when the id is
    /// not a provider owned by `tenant_id`.
    async fn update(
        &self,
        tenant_id: Uuid,
        id: Uuid,
        update: &ProviderUpdate,
    ) -> StoreResult<bool>;
    async fn remove(&self, tenant_id: Uuid, id: Uuid) -> StoreResult<bool>;
    /// Make `id` the tenant's default provider (clearing any previous default).
    /// False when the provider is not owned by `tenant_id`.
    async fn set_default(&self, tenant_id: Uuid, id: Uuid) -> StoreResult<bool>;
    /// Record the last connectivity check (ITEM-051). False when not owned.
    async fn record_check(&self, tenant_id: Uuid, id: Uuid, ok: bool) -> StoreResult<bool>;
    /// Every provider across every tenant, with its decrypted credential,
    /// due-first (never checked, then oldest). The reachability sweep (ITEM-082)
    /// is the only caller — it probes providers no request is touching.
    async fn all_for_probe(&self) -> StoreResult<Vec<(Uuid, ResolvedProvider)>>;
    /// Replace the provider's advertised model set (from its `/models`).
    async fn record_models(&self, provider_id: Uuid, model_ids: &[String]) -> StoreResult<()>;
    /// The models currently cached for a provider (what it last advertised).
    async fn advertised_models(&self, provider_id: Uuid) -> StoreResult<Vec<String>>;
    /// The tenant's provider that advertises `model`, preferring its default
    /// provider. `None` when no provider advertises it.
    async fn provider_for_model(&self, tenant_id: Uuid, model: &str) -> StoreResult<Option<Uuid>>;
    /// Every provider advertising `model`, best-first (default, then oldest).
    /// The adaptive policy uses this to pick the healthiest (ITEM-069).
    async fn providers_for_model(&self, tenant_id: Uuid, model: &str) -> StoreResult<Vec<Uuid>>;
    /// The credential for the request path: the provider that advertises
    /// `model`, else the tenant's default provider. `None` when the tenant has
    /// no provider (shared platform key).
    async fn resolve_for(
        &self,
        tenant_id: Uuid,
        model: Option<&str>,
    ) -> StoreResult<Option<ResolvedProvider>>;
    /// A specific provider's credential (the test-connection probe).
    async fn resolve_provider(
        &self,
        tenant_id: Uuid,
        id: Uuid,
    ) -> StoreResult<Option<ResolvedProvider>>;
}

/// How many providers one reachability pass probes. Bounded so a large
/// deployment degrades to "each provider is checked eventually" rather than a
/// burst of outbound calls.
pub const MAX_PROBES_PER_PASS: usize = 50;

/// Probe each due provider's `GET <base>/models` and record the result
/// (ITEM-082 AC3). Reachability only — never a credential use beyond the probe,
/// never an inference call. A failure is recorded, not retried: the next pass is
/// the retry.
pub async fn run_reachability_pass(store: &dyn ProviderStore, client: &reqwest::Client) {
    let providers = match store.all_for_probe().await {
        Ok(providers) => providers,
        Err(e) => {
            tracing::warn!("provider reachability: could not list providers: {e}");
            return;
        }
    };
    for (tenant_id, provider) in providers.into_iter().take(MAX_PROBES_PER_PASS) {
        let ok = fetch_models(client, &provider).await.is_ok();
        if !ok {
            tracing::debug!("provider {} unreachable", provider.id);
        }
        if let Err(e) = store.record_check(tenant_id, provider.id, ok).await {
            tracing::warn!("provider reachability: could not record check: {e}");
        }
    }
}

// --- Postgres ---------------------------------------------------------------

pub struct PostgresProviderStore {
    pool: sqlx::PgPool,
    secrets: Option<Arc<Secrets>>,
}

impl PostgresProviderStore {
    /// `secrets` is `None` when `PROVIDER_ENCRYPTION_KEY` is unset — provider
    /// provisioning is then disabled (inserts error, resolve returns `None`).
    pub fn new(pool: sqlx::PgPool, secrets: Option<Arc<Secrets>>) -> Self {
        Self { pool, secrets }
    }

    fn encryptor(&self) -> StoreResult<&Secrets> {
        self.secrets
            .as_deref()
            .ok_or_else(|| "provider provisioning is not configured (set PROVIDER_ENCRYPTION_KEY)".into())
    }
}

#[async_trait::async_trait]
impl ProviderStore for PostgresProviderStore {
    async fn list(&self, tenant_id: Uuid) -> StoreResult<Vec<Provider>> {
        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                Uuid,
                String,
                String,
                String,
                String,
                sqlx::types::Json<ProviderMetadata>,
                i64,
                bool,
                Option<String>,
                Option<bool>,
                String,
                String,
            ),
        >(
            "SELECT id, tenant_id, kind, name, base_url, key_hint, metadata,
                    spend_limit_cents, is_default, last_check_at::text, last_check_ok,
                    created_at::text, updated_at::text
             FROM providers WHERE tenant_id = $1
             ORDER BY is_default DESC, created_at",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, tenant_id, kind, name, base_url, key_hint, metadata, spend_limit_cents, is_default, last_check_at, last_check_ok, created_at, updated_at)| {
                    Provider {
                        id,
                        tenant_id,
                        kind,
                        name,
                        base_url,
                        key_hint,
                        metadata: metadata.0,
                        spend_limit_cents,
                        is_default,
                        last_check_at,
                        last_check_ok,
                        created_at,
                        updated_at,
                    }
                },
            )
            .collect())
    }

    async fn insert(&self, tenant_id: Uuid, new: &NewProvider) -> StoreResult<Uuid> {
        let ciphertext = self
            .encryptor()?
            .encrypt(&new.key)
            .ok_or("provider key encryption failed")?;
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO providers (tenant_id, kind, name, base_url, key_ciphertext, key_hint, metadata, spend_limit_cents, is_default)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8,
                     NOT EXISTS (SELECT 1 FROM providers WHERE tenant_id = $1))
             RETURNING id",
        )
        .bind(tenant_id)
        .bind(&new.kind)
        .bind(&new.name)
        .bind(&new.base_url)
        .bind(&ciphertext)
        .bind(key_hint(&new.key))
        .bind(sqlx::types::Json(&new.metadata))
        .bind(new.spend_limit_cents.max(0))
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn update(
        &self,
        tenant_id: Uuid,
        id: Uuid,
        update: &ProviderUpdate,
    ) -> StoreResult<bool> {
        // A rotated key is re-encrypted; leaving it out keeps the stored one.
        let (ciphertext, hint) = match &update.key {
            Some(key) => (
                Some(
                    self.encryptor()?
                        .encrypt(key)
                        .ok_or("provider key encryption failed")?,
                ),
                Some(key_hint(key)),
            ),
            None => (None, None),
        };
        let result = sqlx::query(
            "UPDATE providers SET
                name = COALESCE($3, name),
                base_url = COALESCE($4, base_url),
                key_ciphertext = COALESCE($5, key_ciphertext),
                key_hint = COALESCE($6, key_hint),
                metadata = COALESCE($7, metadata),
                spend_limit_cents = COALESCE($8, spend_limit_cents),
                updated_at = NOW()
             WHERE id = $1 AND tenant_id = $2",
        )
        .bind(id)
        .bind(tenant_id)
        .bind(update.name.as_deref())
        .bind(update.base_url.as_deref())
        .bind(ciphertext)
        .bind(hint)
        .bind(update.metadata.clone().map(sqlx::types::Json))
        .bind(update.spend_limit_cents.map(|c| c.max(0)))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn remove(&self, tenant_id: Uuid, id: Uuid) -> StoreResult<bool> {
        let result = sqlx::query("DELETE FROM providers WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(tenant_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Ok(false);
        }
        // Promote the oldest remaining provider when the default was removed.
        sqlx::query(
            "UPDATE providers SET is_default = TRUE
             WHERE id = (SELECT id FROM providers WHERE tenant_id = $1 ORDER BY created_at LIMIT 1)
               AND NOT EXISTS (SELECT 1 FROM providers WHERE tenant_id = $1 AND is_default)",
        )
        .bind(tenant_id)
        .execute(&self.pool)
        .await?;
        Ok(true)
    }

    async fn set_default(&self, tenant_id: Uuid, id: Uuid) -> StoreResult<bool> {
        let mut tx = self.pool.begin().await?;
        let exists: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM providers WHERE id = $1 AND tenant_id = $2")
                .bind(id)
                .bind(tenant_id)
                .fetch_optional(&mut *tx)
                .await?;
        if exists.is_none() {
            return Ok(false);
        }
        sqlx::query(
            "UPDATE providers SET is_default = FALSE
             WHERE tenant_id = $1 AND id <> $2 AND is_default",
        )
        .bind(tenant_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE providers SET is_default = TRUE WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(tenant_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    async fn record_check(&self, tenant_id: Uuid, id: Uuid, ok: bool) -> StoreResult<bool> {
        let result = sqlx::query(
            "UPDATE providers SET last_check_at = NOW(), last_check_ok = $3
             WHERE id = $1 AND tenant_id = $2",
        )
        .bind(id)
        .bind(tenant_id)
        .bind(ok)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn all_for_probe(&self) -> StoreResult<Vec<(Uuid, ResolvedProvider)>> {
        let Some(secrets) = self.secrets.as_deref() else {
            return Ok(Vec::new());
        };
        let rows: Vec<(
            Uuid,
            Uuid,
            String,
            String,
            String,
            sqlx::types::Json<ProviderMetadata>,
        )> = sqlx::query_as(
            "SELECT id, tenant_id, kind, base_url, key_ciphertext, metadata FROM providers
             ORDER BY last_check_at NULLS FIRST, created_at",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, tenant_id, kind, base_url, ciphertext, metadata)| {
                let api_key = secrets.decrypt(&ciphertext)?;
                Some((
                    tenant_id,
                    ResolvedProvider {
                        id,
                        kind,
                        base_url,
                        api_key,
                        metadata: metadata.0,
                    },
                ))
            })
            .collect())
    }

    async fn record_models(&self, provider_id: Uuid, model_ids: &[String]) -> StoreResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM provider_models WHERE provider_id = $1")
            .bind(provider_id)
            .execute(&mut *tx)
            .await?;
        if !model_ids.is_empty() {
            sqlx::query(
                "INSERT INTO provider_models (provider_id, model_id)
                 SELECT $1, UNNEST($2::text[])
                 ON CONFLICT (provider_id, model_id) DO UPDATE SET discovered_at = NOW()",
            )
            .bind(provider_id)
            .bind(model_ids)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn advertised_models(&self, provider_id: Uuid) -> StoreResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT model_id FROM provider_models WHERE provider_id = $1 ORDER BY model_id",
        )
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    async fn provider_for_model(&self, tenant_id: Uuid, model: &str) -> StoreResult<Option<Uuid>> {
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT p.id FROM providers p
             JOIN provider_models pm ON pm.provider_id = p.id
             WHERE p.tenant_id = $1 AND pm.model_id = $2
             ORDER BY p.is_default DESC, p.created_at
             LIMIT 1",
        )
        .bind(tenant_id)
        .bind(model)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id,)| id))
    }

    async fn providers_for_model(&self, tenant_id: Uuid, model: &str) -> StoreResult<Vec<Uuid>> {
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT p.id FROM providers p
             JOIN provider_models pm ON pm.provider_id = p.id
             WHERE p.tenant_id = $1 AND pm.model_id = $2
             ORDER BY p.is_default DESC, p.created_at",
        )
        .bind(tenant_id)
        .bind(model)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    async fn resolve_for(
        &self,
        tenant_id: Uuid,
        model: Option<&str>,
    ) -> StoreResult<Option<ResolvedProvider>> {
        let Some(secrets) = self.secrets.as_deref() else {
            return Ok(None);
        };
        // Prefer the provider that advertises the requested model; else the
        // tenant's default provider, then the oldest.
        let chosen_id = match model {
            Some(model) => self.provider_for_model(tenant_id, model).await?,
            None => None,
        };
        let row: Option<(Uuid, String, String, String, sqlx::types::Json<ProviderMetadata>)> =
            if let Some(id) = chosen_id {
                sqlx::query_as(
                    "SELECT id, kind, base_url, key_ciphertext, metadata FROM providers
                     WHERE id = $1 AND tenant_id = $2",
                )
                .bind(id)
                .bind(tenant_id)
                .fetch_optional(&self.pool)
                .await?
            } else {
                sqlx::query_as(
                    "SELECT id, kind, base_url, key_ciphertext, metadata FROM providers
                     WHERE tenant_id = $1 ORDER BY is_default DESC, created_at LIMIT 1",
                )
                .bind(tenant_id)
                .fetch_optional(&self.pool)
                .await?
            };
        let Some((id, kind, base_url, ciphertext, metadata)) = row else {
            return Ok(None);
        };
        match secrets.decrypt(&ciphertext) {
            Some(api_key) => Ok(Some(ResolvedProvider {
                id,
                kind,
                base_url,
                api_key,
                metadata: metadata.0,
            })),
            None => {
                tracing::error!(
                    "provider key for tenant {tenant_id} could not be decrypted (wrong PROVIDER_ENCRYPTION_KEY?)"
                );
                Ok(None)
            }
        }
    }

    async fn resolve_provider(
        &self,
        tenant_id: Uuid,
        id: Uuid,
    ) -> StoreResult<Option<ResolvedProvider>> {
        let Some(secrets) = self.secrets.as_deref() else {
            return Ok(None);
        };
        let row: Option<(Uuid, String, String, String, sqlx::types::Json<ProviderMetadata>)> = sqlx::query_as(
            "SELECT id, kind, base_url, key_ciphertext, metadata FROM providers WHERE id = $1 AND tenant_id = $2",
        )
        .bind(id)
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((id, kind, base_url, ciphertext, metadata)) = row else {
            return Ok(None);
        };
        Ok(secrets
            .decrypt(&ciphertext)
            .map(|api_key| ResolvedProvider {
                id,
                kind,
                base_url,
                api_key,
                metadata: metadata.0,
            }))
    }
}

// --- In-memory (tests) ------------------------------------------------------

struct InMemoryRow {
    provider: Provider,
    api_key: String,
}

#[derive(Default)]
pub struct InMemoryProviderStore {
    rows: Mutex<Vec<InMemoryRow>>,
    /// provider id -> advertised model ids (the `/models` cache).
    models: Mutex<std::collections::HashMap<Uuid, Vec<String>>>,
}

impl InMemoryProviderStore {
    /// Seed a ready-to-resolve provider (raw key held in memory — test only).
    pub fn with_provider(tenant_id: Uuid, base_url: &str, api_key: &str) -> Self {
        let store = Self::default();
        store.rows.lock().unwrap().push(InMemoryRow {
            provider: Provider {
                id: Uuid::new_v4(),
                tenant_id,
                kind: "deepseek".to_string(),
                name: "seeded".to_string(),
                base_url: base_url.to_string(),
                key_hint: key_hint(api_key),
                metadata: ProviderMetadata::default(),
                spend_limit_cents: 0,
                is_default: true,
                last_check_at: None,
                last_check_ok: None,
                created_at: "0".to_string(),
                updated_at: "0".to_string(),
            },
            api_key: api_key.to_string(),
        });
        store
    }
}

#[async_trait::async_trait]
impl ProviderStore for InMemoryProviderStore {
    async fn list(&self, tenant_id: Uuid) -> StoreResult<Vec<Provider>> {
        let mut providers: Vec<Provider> = self
            .rows
            .lock()
            .unwrap()
            .iter()
            .filter(|row| row.provider.tenant_id == tenant_id)
            .map(|row| row.provider.clone())
            .collect();
        // Default first, then oldest — matches the Postgres ordering.
        providers.sort_by_key(|provider| (!provider.is_default, provider.created_at.clone()));
        Ok(providers)
    }

    async fn insert(&self, tenant_id: Uuid, new: &NewProvider) -> StoreResult<Uuid> {
        let id = Uuid::new_v4();
        let mut rows = self.rows.lock().unwrap();
        let is_default = !rows.iter().any(|row| row.provider.tenant_id == tenant_id);
        rows.push(InMemoryRow {
            provider: Provider {
                id,
                tenant_id,
                kind: new.kind.clone(),
                name: new.name.clone(),
                base_url: new.base_url.clone(),
                key_hint: key_hint(&new.key),
                metadata: new.metadata.clone(),
                spend_limit_cents: new.spend_limit_cents.max(0),
                is_default,
                last_check_at: None,
                last_check_ok: None,
                created_at: "0".to_string(),
                updated_at: "0".to_string(),
            },
            api_key: new.key.clone(),
        });
        Ok(id)
    }

    async fn update(
        &self,
        tenant_id: Uuid,
        id: Uuid,
        update: &ProviderUpdate,
    ) -> StoreResult<bool> {
        let mut rows = self.rows.lock().unwrap();
        let Some(row) = rows
            .iter_mut()
            .find(|row| row.provider.id == id && row.provider.tenant_id == tenant_id)
        else {
            return Ok(false);
        };
        if let Some(name) = &update.name {
            row.provider.name = name.clone();
        }
        if let Some(base_url) = &update.base_url {
            row.provider.base_url = base_url.clone();
        }
        if let Some(key) = &update.key {
            row.provider.key_hint = key_hint(key);
            row.api_key = key.clone();
        }
        if let Some(metadata) = &update.metadata {
            row.provider.metadata = metadata.clone();
        }
        if let Some(limit) = update.spend_limit_cents {
            row.provider.spend_limit_cents = limit.max(0);
        }
        row.provider.updated_at = "1".to_string();
        Ok(true)
    }

    async fn remove(&self, tenant_id: Uuid, id: Uuid) -> StoreResult<bool> {
        let mut rows = self.rows.lock().unwrap();
        let before = rows.len();
        rows.retain(|row| !(row.provider.id == id && row.provider.tenant_id == tenant_id));
        if rows.len() == before {
            return Ok(false);
        }
        self.models.lock().unwrap().remove(&id);
        // Promote the oldest remaining provider when the default was removed.
        let has_default = rows
            .iter()
            .any(|row| row.provider.tenant_id == tenant_id && row.provider.is_default);
        if !has_default {
            if let Some(row) = rows
                .iter_mut()
                .filter(|row| row.provider.tenant_id == tenant_id)
                .min_by_key(|row| row.provider.created_at.clone())
            {
                row.provider.is_default = true;
            }
        }
        Ok(true)
    }

    async fn set_default(&self, tenant_id: Uuid, id: Uuid) -> StoreResult<bool> {
        let mut rows = self.rows.lock().unwrap();
        if !rows
            .iter()
            .any(|row| row.provider.id == id && row.provider.tenant_id == tenant_id)
        {
            return Ok(false);
        }
        for row in rows.iter_mut().filter(|row| row.provider.tenant_id == tenant_id) {
            row.provider.is_default = row.provider.id == id;
        }
        Ok(true)
    }

    async fn record_check(&self, tenant_id: Uuid, id: Uuid, ok: bool) -> StoreResult<bool> {
        let mut rows = self.rows.lock().unwrap();
        match rows
            .iter_mut()
            .find(|row| row.provider.id == id && row.provider.tenant_id == tenant_id)
        {
            Some(row) => {
                row.provider.last_check_at = Some("1".to_string());
                row.provider.last_check_ok = Some(ok);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn all_for_probe(&self) -> StoreResult<Vec<(Uuid, ResolvedProvider)>> {
        let rows = self.rows.lock().unwrap();
        Ok(rows
            .iter()
            .map(|row| {
                (
                    row.provider.tenant_id,
                    ResolvedProvider {
                        id: row.provider.id,
                        kind: row.provider.kind.clone(),
                        base_url: row.provider.base_url.clone(),
                        api_key: row.api_key.clone(),
                        metadata: row.provider.metadata.clone(),
                    },
                )
            })
            .collect())
    }

    async fn record_models(&self, provider_id: Uuid, model_ids: &[String]) -> StoreResult<()> {
        self.models
            .lock()
            .unwrap()
            .insert(provider_id, model_ids.to_vec());
        Ok(())
    }

    async fn advertised_models(&self, provider_id: Uuid) -> StoreResult<Vec<String>> {
        Ok(self
            .models
            .lock()
            .unwrap()
            .get(&provider_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn provider_for_model(&self, tenant_id: Uuid, model: &str) -> StoreResult<Option<Uuid>> {
        let rows = self.rows.lock().unwrap();
        let models = self.models.lock().unwrap();
        Ok(rows
            .iter()
            .filter(|row| {
                row.provider.tenant_id == tenant_id
                    && models
                        .get(&row.provider.id)
                        .is_some_and(|ids| ids.iter().any(|id| id == model))
            })
            .min_by_key(|row| (!row.provider.is_default, row.provider.created_at.clone()))
            .map(|row| row.provider.id))
    }

    async fn providers_for_model(&self, tenant_id: Uuid, model: &str) -> StoreResult<Vec<Uuid>> {
        let rows = self.rows.lock().unwrap();
        let models = self.models.lock().unwrap();
        let mut candidates: Vec<(bool, String, Uuid)> = rows
            .iter()
            .filter(|row| {
                row.provider.tenant_id == tenant_id
                    && models
                        .get(&row.provider.id)
                        .is_some_and(|ids| ids.iter().any(|id| id == model))
            })
            .map(|row| {
                (
                    !row.provider.is_default,
                    row.provider.created_at.clone(),
                    row.provider.id,
                )
            })
            .collect();
        candidates.sort();
        Ok(candidates.into_iter().map(|(_, _, id)| id).collect())
    }

    async fn resolve_for(
        &self,
        tenant_id: Uuid,
        model: Option<&str>,
    ) -> StoreResult<Option<ResolvedProvider>> {
        let chosen_id = match model {
            Some(model) => self.provider_for_model(tenant_id, model).await?,
            None => None,
        };
        let rows = self.rows.lock().unwrap();
        let chosen = if let Some(id) = chosen_id {
            rows.iter().find(|row| row.provider.id == id)
        } else {
            rows.iter()
                .filter(|row| row.provider.tenant_id == tenant_id)
                .min_by_key(|row| (!row.provider.is_default, row.provider.created_at.clone()))
        };
        Ok(chosen.map(|row| ResolvedProvider {
            id: row.provider.id,
            kind: row.provider.kind.clone(),
            base_url: row.provider.base_url.clone(),
            api_key: row.api_key.clone(),
            metadata: row.provider.metadata.clone(),
        }))
    }

    async fn resolve_provider(
        &self,
        tenant_id: Uuid,
        id: Uuid,
    ) -> StoreResult<Option<ResolvedProvider>> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .find(|row| row.provider.tenant_id == tenant_id && row.provider.id == id)
            .map(|row| ResolvedProvider {
                id: row.provider.id,
                kind: row.provider.kind.clone(),
                base_url: row.provider.base_url.clone(),
                api_key: row.api_key.clone(),
                metadata: row.provider.metadata.clone(),
            }))
    }
}

#[cfg(test)]
mod tests;
