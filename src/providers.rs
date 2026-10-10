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

pub type StoreResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

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
    pub base_url: String,
    pub api_key: String,
    pub metadata: ProviderMetadata,
}

/// Supported provider kinds. Anthropic is not OpenAI-shaped and is not wired.
pub const PROVIDER_KINDS: [&str; 3] = ["deepseek", "openai", "openai-compatible"];

pub fn is_supported_kind(kind: &str) -> bool {
    PROVIDER_KINDS.contains(&kind)
}

/// A human name when the tenant does not supply one.
pub fn default_name(kind: &str) -> String {
    match kind {
        "deepseek" => "DeepSeek".to_string(),
        "openai" => "OpenAI".to_string(),
        _ => "Custom provider".to_string(),
    }
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
    /// Replace the provider's advertised model set (from its `/models`).
    async fn record_models(&self, provider_id: Uuid, model_ids: &[String]) -> StoreResult<()>;
    /// The models currently cached for a provider (what it last advertised).
    async fn advertised_models(&self, provider_id: Uuid) -> StoreResult<Vec<String>>;
    /// The tenant's provider that advertises `model`, preferring its default
    /// provider. `None` when no provider advertises it.
    async fn provider_for_model(&self, tenant_id: Uuid, model: &str) -> StoreResult<Option<Uuid>>;
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
                String,
                String,
            ),
        >(
            "SELECT id, tenant_id, kind, name, base_url, key_hint, metadata,
                    spend_limit_cents, is_default, created_at::text, updated_at::text
             FROM providers WHERE tenant_id = $1
             ORDER BY is_default DESC, created_at",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, tenant_id, kind, name, base_url, key_hint, metadata, spend_limit_cents, is_default, created_at, updated_at)| {
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
        let row: Option<(Uuid, String, String, sqlx::types::Json<ProviderMetadata>)> =
            if let Some(id) = chosen_id {
                sqlx::query_as(
                    "SELECT id, base_url, key_ciphertext, metadata FROM providers
                     WHERE id = $1 AND tenant_id = $2",
                )
                .bind(id)
                .bind(tenant_id)
                .fetch_optional(&self.pool)
                .await?
            } else {
                sqlx::query_as(
                    "SELECT id, base_url, key_ciphertext, metadata FROM providers
                     WHERE tenant_id = $1 ORDER BY is_default DESC, created_at LIMIT 1",
                )
                .bind(tenant_id)
                .fetch_optional(&self.pool)
                .await?
            };
        let Some((id, base_url, ciphertext, metadata)) = row else {
            return Ok(None);
        };
        match secrets.decrypt(&ciphertext) {
            Some(api_key) => Ok(Some(ResolvedProvider {
                id,
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
        let row: Option<(Uuid, String, String, sqlx::types::Json<ProviderMetadata>)> = sqlx::query_as(
            "SELECT id, base_url, key_ciphertext, metadata FROM providers WHERE id = $1 AND tenant_id = $2",
        )
        .bind(id)
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((id, base_url, ciphertext, metadata)) = row else {
            return Ok(None);
        };
        Ok(secrets
            .decrypt(&ciphertext)
            .map(|api_key| ResolvedProvider {
                id,
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
                base_url: row.provider.base_url.clone(),
                api_key: row.api_key.clone(),
                metadata: row.provider.metadata.clone(),
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_show_only_the_tail() {
        assert_eq!(key_hint("sk-1234567890ab12"), "\u{2026}ab12");
        assert_eq!(key_hint("ab"), "\u{2026}ab");
        assert_eq!(key_hint(""), "\u{2026}");
    }

    #[test]
    fn kinds_are_allowlisted() {
        assert!(is_supported_kind("deepseek"));
        assert!(is_supported_kind("openai-compatible"));
        assert!(!is_supported_kind("anthropic"));
        assert!(!is_supported_kind("garbage"));
    }

    #[tokio::test]
    async fn in_memory_store_round_trips() {
        let tenant = Uuid::new_v4();
        let store = InMemoryProviderStore::default();
        let id = store
            .insert(
                tenant,
                &NewProvider {
                    kind: "deepseek".to_string(),
                    name: "My DeepSeek".to_string(),
                    base_url: "https://api.deepseek.com/v1".to_string(),
                    key: "sk-mine-ab12".to_string(),
                    metadata: ProviderMetadata {
                        org: Some("org-1".to_string()),
                        headers: [("X-Trace".to_string(), "on".to_string())].into(),
                    },
                    spend_limit_cents: 0,
                },
            )
            .await
            .expect("insert");

        let listed = store.list(tenant).await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key_hint, "\u{2026}ab12");
        assert!(!listed[0].key_hint.contains("sk-mine"));
        assert_eq!(listed[0].metadata.org.as_deref(), Some("org-1"));

        let resolved = store.resolve_for(tenant, None).await.expect("resolve").expect("configured");
        assert_eq!(resolved.base_url, "https://api.deepseek.com/v1");
        assert_eq!(resolved.api_key, "sk-mine-ab12");
        assert_eq!(resolved.metadata.headers["X-Trace"], "on");

        assert!(store.remove(tenant, id).await.expect("remove"));
        assert!(store.resolve_for(tenant, None).await.expect("resolve").is_none());
    }

    #[tokio::test]
    async fn rotation_updates_the_key_without_recreating_the_row() {
        // ITEM-043 AC5: a provider can be rotated in place; the old id survives
        // and the new key resolves on the request path.
        let tenant = Uuid::new_v4();
        let store = InMemoryProviderStore::default();
        let id = store
            .insert(
                tenant,
                &NewProvider {
                    kind: "openai".to_string(),
                    name: "Mine".to_string(),
                    base_url: "https://api.openai.com/v1".to_string(),
                    key: "sk-old-1111".to_string(),
                    metadata: ProviderMetadata::default(),
                    spend_limit_cents: 0,
                },
            )
            .await
            .expect("insert");

        let rotated = store
            .update(
                tenant,
                id,
                &ProviderUpdate {
                    name: Some("Renamed".to_string()),
                    base_url: None,
                    key: Some("sk-new-2222".to_string()),
                    metadata: Some(ProviderMetadata {
                        org: Some("org-2".to_string()),
                        headers: BTreeMap::new(),
                    }),
                    spend_limit_cents: Some(2500),
                },
            )
            .await
            .expect("update");
        assert!(rotated);

        let listed = store.list(tenant).await.expect("list");
        assert_eq!(listed.len(), 1, "rotation does not create a new row");
        assert_eq!(listed[0].id, id);
        assert_eq!(listed[0].name, "Renamed");
        assert_eq!(listed[0].key_hint, "\u{2026}2222");
        assert_eq!(listed[0].metadata.org.as_deref(), Some("org-2"));
        assert_eq!(listed[0].spend_limit_cents, 2500, "ITEM-047 limit is stored");
        let resolved = store.resolve_for(tenant, None).await.expect("resolve").expect("configured");
        assert_eq!(resolved.api_key, "sk-new-2222", "the rotated key is used");

        // Another tenant cannot rotate it.
        assert!(!store
            .update(Uuid::new_v4(), id, &ProviderUpdate { key: Some("sk-thief".to_string()), ..Default::default() })
            .await
            .expect("update"));
    }

    #[tokio::test]
    async fn the_first_provider_is_default_and_set_default_switches() {
        let tenant = Uuid::new_v4();
        let store = InMemoryProviderStore::default();
        let new = |kind: &str, base_url: &str, key: &str| NewProvider {
            kind: kind.to_string(),
            name: kind.to_string(),
            base_url: base_url.to_string(),
            key: key.to_string(),
            metadata: ProviderMetadata::default(),
            spend_limit_cents: 0,
        };
        let a = store
            .insert(tenant, &new("deepseek", "https://a.test/v1", "sk-a"))
            .await
            .unwrap();
        let b = store
            .insert(tenant, &new("openai", "https://b.test/v1", "sk-b"))
            .await
            .unwrap();

        let is_default = |providers: &[Provider], id| {
            providers.iter().find(|p| p.id == id).unwrap().is_default
        };
        let listed = store.list(tenant).await.unwrap();
        assert!(is_default(&listed, a), "the first provider is the default");
        assert!(!is_default(&listed, b));

        assert!(store.set_default(tenant, b).await.unwrap());
        let listed = store.list(tenant).await.unwrap();
        assert!(is_default(&listed, b), "set_default switches the default");
        assert!(!is_default(&listed, a));
        // Another tenant cannot set it, and the ordering is default-first.
        assert!(!store.set_default(Uuid::new_v4(), a).await.unwrap());
        assert_eq!(store.list(tenant).await.unwrap()[0].id, b, "default sorts first");

        // Removing the default promotes the oldest remaining provider.
        assert!(store.remove(tenant, b).await.unwrap());
        let listed = store.list(tenant).await.unwrap();
        assert!(is_default(&listed, a), "a new default is promoted on removal");
    }

    #[tokio::test]
    async fn resolve_for_falls_back_to_the_default_for_unadvertised_models() {
        let tenant = Uuid::new_v4();
        let store = InMemoryProviderStore::default();
        let new = |kind: &str, base_url: &str, key: &str| NewProvider {
            kind: kind.to_string(),
            name: kind.to_string(),
            base_url: base_url.to_string(),
            key: key.to_string(),
            metadata: ProviderMetadata::default(),
            spend_limit_cents: 0,
        };
        let deep = store
            .insert(tenant, &new("deepseek", "https://deep.test/v1", "sk-deep"))
            .await
            .unwrap();
        let catch_all = store
            .insert(tenant, &new("openai-compatible", "https://catch.test/v1", "sk-catch"))
            .await
            .unwrap();
        store
            .record_models(deep, &["deepseek-chat".to_string()])
            .await
            .unwrap();
        store
            .record_models(catch_all, &["my-custom-model".to_string()])
            .await
            .unwrap();
        // Make the catch-all the default.
        assert!(store.set_default(tenant, catch_all).await.unwrap());
        // A model the deepseek provider advertises routes there...
        let owned = store.resolve_for(tenant, Some("deepseek-chat")).await.unwrap().unwrap();
        assert_eq!(owned.id, deep);
        // ...an advertised custom model routes to its provider...
        let free = store.resolve_for(tenant, Some("my-custom-model")).await.unwrap().unwrap();
        assert_eq!(free.id, catch_all);
        // ...and a model nobody advertises falls back to the default.
        let unknown = store
            .resolve_for(tenant, Some("nobody-advertises-this"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(unknown.id, catch_all);
    }

    #[tokio::test]
    async fn resolve_for_picks_the_provider_that_advertises_the_model() {
        let tenant = Uuid::new_v4();
        let store = InMemoryProviderStore::default();
        let new = |kind: &str, base_url: &str, key: &str| NewProvider {
            kind: kind.to_string(),
            name: kind.to_string(),
            base_url: base_url.to_string(),
            key: key.to_string(),
            metadata: ProviderMetadata::default(),
            spend_limit_cents: 0,
        };
        let deep = store
            .insert(tenant, &new("deepseek", "https://deep.test/v1", "sk-deep"))
            .await
            .unwrap();
        let open = store
            .insert(tenant, &new("openai-compatible", "https://open.test/v1", "sk-open"))
            .await
            .unwrap();
        store
            .record_models(deep, &["deepseek-chat".to_string()])
            .await
            .unwrap();
        store
            .record_models(open, &["gpt-4o".to_string()])
            .await
            .unwrap();

        let route = store.resolve_for(tenant, Some("gpt-4o")).await.unwrap().unwrap();
        assert_eq!(route.id, open, "gpt-4o is advertised by the second provider");
        let route = store.resolve_for(tenant, Some("deepseek-chat")).await.unwrap().unwrap();
        assert_eq!(route.id, deep);
        // An unadvertised model (or no hint) falls back to the default (first).
        let route = store.resolve_for(tenant, Some("mystery")).await.unwrap().unwrap();
        assert_eq!(route.id, deep);
        let route = store.resolve_for(tenant, None).await.unwrap().unwrap();
        assert_eq!(route.id, deep);
        assert_eq!(
            store.advertised_models(open).await.unwrap(),
            vec!["gpt-4o".to_string()]
        );
    }

    #[tokio::test]
    async fn tenant_scoping_is_enforced() {
        let tenant = Uuid::new_v4();
        let other = Uuid::new_v4();
        let store = InMemoryProviderStore::with_provider(tenant, "https://x/v1", "sk-x");
        assert!(store.list(other).await.expect("list").is_empty());
        assert!(store.resolve_for(other, None).await.expect("resolve").is_none());
    }

    #[tokio::test]
    async fn resolve_provider_targets_one_tenant_owned_row() {
        let tenant = Uuid::new_v4();
        let other = Uuid::new_v4();
        let store = InMemoryProviderStore::with_provider(tenant, "https://mine/v1", "sk-mine");
        let id = store.list(tenant).await.expect("list")[0].id;

        let resolved = store.resolve_provider(tenant, id).await.expect("resolve").expect("found");
        assert_eq!(resolved.base_url, "https://mine/v1");
        assert_eq!(resolved.api_key, "sk-mine");
        // Another tenant can never resolve it, even with the id.
        assert!(store.resolve_provider(other, id).await.expect("resolve").is_none());
    }
}
