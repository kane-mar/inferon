//! Provider provisioning (ITEM-043): a tenant's own upstream credentials.
//!
//! The `ProviderStore` seam sits between the proxy/dashboard and Postgres.
//! `PostgresProviderStore` encrypts keys at rest ([`crate::secret::Secrets`]) and
//! only ever exposes a masked `key_hint` on reads; `resolve` returns the
//! decrypted credential for the request path. `InMemoryProviderStore` is the
//! test double.

use crate::secret::Secrets;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub type StoreResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

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
    pub created_at: String,
}

/// A provider to register (`key` is the raw upstream credential).
#[derive(Clone, Debug)]
pub struct NewProvider {
    pub kind: String,
    pub name: String,
    pub base_url: String,
    pub key: String,
}

/// The decrypted credential the proxy uses for a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedProvider {
    pub base_url: String,
    pub api_key: String,
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
    async fn remove(&self, tenant_id: Uuid, id: Uuid) -> StoreResult<bool>;
    /// The tenant's credential for the request path, if one is configured.
    async fn resolve(&self, tenant_id: Uuid) -> StoreResult<Option<ResolvedProvider>>;
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
        let rows = sqlx::query_as::<_, (Uuid, Uuid, String, String, String, String, String)>(
            "SELECT id, tenant_id, kind, name, base_url, key_hint, created_at::text
             FROM providers WHERE tenant_id = $1 ORDER BY created_at",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, tenant_id, kind, name, base_url, key_hint, created_at)| Provider {
                id,
                tenant_id,
                kind,
                name,
                base_url,
                key_hint,
                created_at,
            })
            .collect())
    }

    async fn insert(&self, tenant_id: Uuid, new: &NewProvider) -> StoreResult<Uuid> {
        let ciphertext = self
            .encryptor()?
            .encrypt(&new.key)
            .ok_or("provider key encryption failed")?;
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO providers (tenant_id, kind, name, base_url, key_ciphertext, key_hint)
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
        )
        .bind(tenant_id)
        .bind(&new.kind)
        .bind(&new.name)
        .bind(&new.base_url)
        .bind(&ciphertext)
        .bind(key_hint(&new.key))
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn remove(&self, tenant_id: Uuid, id: Uuid) -> StoreResult<bool> {
        let result = sqlx::query("DELETE FROM providers WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(tenant_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn resolve(&self, tenant_id: Uuid) -> StoreResult<Option<ResolvedProvider>> {
        let Some(secrets) = self.secrets.as_deref() else {
            return Ok(None);
        };
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT base_url, key_ciphertext FROM providers
             WHERE tenant_id = $1 ORDER BY created_at LIMIT 1",
        )
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((base_url, ciphertext)) = row else {
            return Ok(None);
        };
        match secrets.decrypt(&ciphertext) {
            Some(api_key) => Ok(Some(ResolvedProvider { base_url, api_key })),
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
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT base_url, key_ciphertext FROM providers WHERE id = $1 AND tenant_id = $2",
        )
        .bind(id)
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((base_url, ciphertext)) = row else {
            return Ok(None);
        };
        Ok(secrets
            .decrypt(&ciphertext)
            .map(|api_key| ResolvedProvider { base_url, api_key }))
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
                created_at: "0".to_string(),
            },
            api_key: api_key.to_string(),
        });
        store
    }
}

#[async_trait::async_trait]
impl ProviderStore for InMemoryProviderStore {
    async fn list(&self, tenant_id: Uuid) -> StoreResult<Vec<Provider>> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .filter(|row| row.provider.tenant_id == tenant_id)
            .map(|row| row.provider.clone())
            .collect())
    }

    async fn insert(&self, tenant_id: Uuid, new: &NewProvider) -> StoreResult<Uuid> {
        let id = Uuid::new_v4();
        self.rows.lock().unwrap().push(InMemoryRow {
            provider: Provider {
                id,
                tenant_id,
                kind: new.kind.clone(),
                name: new.name.clone(),
                base_url: new.base_url.clone(),
                key_hint: key_hint(&new.key),
                created_at: "0".to_string(),
            },
            api_key: new.key.clone(),
        });
        Ok(id)
    }

    async fn remove(&self, tenant_id: Uuid, id: Uuid) -> StoreResult<bool> {
        let mut rows = self.rows.lock().unwrap();
        let before = rows.len();
        rows.retain(|row| !(row.provider.id == id && row.provider.tenant_id == tenant_id));
        Ok(rows.len() < before)
    }

    async fn resolve(&self, tenant_id: Uuid) -> StoreResult<Option<ResolvedProvider>> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .find(|row| row.provider.tenant_id == tenant_id)
            .map(|row| ResolvedProvider {
                base_url: row.provider.base_url.clone(),
                api_key: row.api_key.clone(),
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
                base_url: row.provider.base_url.clone(),
                api_key: row.api_key.clone(),
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
                },
            )
            .await
            .expect("insert");

        let listed = store.list(tenant).await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key_hint, "\u{2026}ab12");
        assert!(!listed[0].key_hint.contains("sk-mine"));

        let resolved = store.resolve(tenant).await.expect("resolve").expect("configured");
        assert_eq!(resolved.base_url, "https://api.deepseek.com/v1");
        assert_eq!(resolved.api_key, "sk-mine-ab12");

        assert!(store.remove(tenant, id).await.expect("remove"));
        assert!(store.resolve(tenant).await.expect("resolve").is_none());
    }

    #[tokio::test]
    async fn tenant_scoping_is_enforced() {
        let tenant = Uuid::new_v4();
        let other = Uuid::new_v4();
        let store = InMemoryProviderStore::with_provider(tenant, "https://x/v1", "sk-x");
        assert!(store.list(other).await.expect("list").is_empty());
        assert!(store.resolve(other).await.expect("resolve").is_none());
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
