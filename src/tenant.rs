//! Tenant + API-key persistence (ITEM-034): the `TenantStore` seam and its
//! two implementations — `InMemoryTenantStore` (unit tests) and
//! `PostgresTenantStore` (production: tenants + api_keys rows).
//!
//! Client API keys are only ever stored/compared as SHA-256 hashes; the
//! upstream master key is Inferon-held and never returned to clients.

use std::collections::HashMap;
use uuid::Uuid;

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
