//! Per-provider model selection (ITEM-044).
//!
//! There is no static catalog: the available models are whatever each provider
//! advertises at `GET <base>/models` (discovered and cached in `provider_models`).
//! [`ModelStore`] only holds the per-provider enable/disable + default flags —
//! a model with no row is unrestricted (enabled). Prices are applied by the cost
//! engine ([`crate::cost::price_for`]) when a request is billed.

use uuid::Uuid;

pub type StoreResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// One model's flags for a provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelSetting {
    pub model_id: String,
    pub enabled: bool,
    pub is_default: bool,
}

#[async_trait::async_trait]
pub trait ModelStore: Send + Sync {
    async fn settings(&self, provider_id: Uuid) -> StoreResult<Vec<ModelSetting>>;
    async fn set_enabled(&self, provider_id: Uuid, model_id: &str, enabled: bool) -> StoreResult<()>;
    /// Mark one of the provider's models the default (there is at most one).
    async fn set_default(&self, provider_id: Uuid, model_id: &str) -> StoreResult<()>;
    /// True only when the provider has **explicitly** disabled the model. The
    /// shared platform key (`None`) is never disabled.
    async fn is_disabled(&self, provider_id: Option<Uuid>, model_id: &str) -> StoreResult<bool>;
}

// --- Postgres ---------------------------------------------------------------

pub struct PostgresModelStore {
    pool: sqlx::PgPool,
}

impl PostgresModelStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl ModelStore for PostgresModelStore {
    async fn settings(&self, provider_id: Uuid) -> StoreResult<Vec<ModelSetting>> {
        let rows = sqlx::query_as::<_, (String, bool, bool)>(
            "SELECT model_id, enabled, is_default FROM tenant_models
             WHERE provider_id = $1 ORDER BY model_id",
        )
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(model_id, enabled, is_default)| ModelSetting {
                model_id,
                enabled,
                is_default,
            })
            .collect())
    }

    async fn set_enabled(&self, provider_id: Uuid, model_id: &str, enabled: bool) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO tenant_models (provider_id, model_id, enabled) VALUES ($1, $2, $3)
             ON CONFLICT (provider_id, model_id)
             DO UPDATE SET enabled = $3, updated_at = NOW()",
        )
        .bind(provider_id)
        .bind(model_id)
        .bind(enabled)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_default(&self, provider_id: Uuid, model_id: &str) -> StoreResult<()> {
        // One default per provider: clear its other defaults first.
        sqlx::query(
            "UPDATE tenant_models SET is_default = FALSE, updated_at = NOW()
             WHERE provider_id = $1 AND is_default",
        )
        .bind(provider_id)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO tenant_models (provider_id, model_id, enabled, is_default)
             VALUES ($1, $2, TRUE, TRUE)
             ON CONFLICT (provider_id, model_id)
             DO UPDATE SET is_default = TRUE, enabled = TRUE, updated_at = NOW()",
        )
        .bind(provider_id)
        .bind(model_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn is_disabled(&self, provider_id: Option<Uuid>, model_id: &str) -> StoreResult<bool> {
        let Some(provider_id) = provider_id else {
            return Ok(false);
        };
        let row: Option<(bool,)> = sqlx::query_as(
            "SELECT enabled FROM tenant_models WHERE provider_id = $1 AND model_id = $2",
        )
        .bind(provider_id)
        .bind(model_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(matches!(row, Some((false,))))
    }
}

// --- In-memory (tests) ------------------------------------------------------

#[derive(Default)]
pub struct InMemoryModelStore {
    rows: std::sync::Mutex<Vec<ModelSettingRow>>,
}

struct ModelSettingRow {
    provider_id: Uuid,
    setting: ModelSetting,
}

impl InMemoryModelStore {
    pub fn seed(provider_id: Uuid, model_id: &str, enabled: bool, is_default: bool) -> Self {
        let store = Self::default();
        store.rows.lock().unwrap().push(ModelSettingRow {
            provider_id,
            setting: ModelSetting {
                model_id: model_id.to_string(),
                enabled,
                is_default,
            },
        });
        store
    }
}

#[async_trait::async_trait]
impl ModelStore for InMemoryModelStore {
    async fn settings(&self, provider_id: Uuid) -> StoreResult<Vec<ModelSetting>> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .filter(|row| row.provider_id == provider_id)
            .map(|row| row.setting.clone())
            .collect())
    }

    async fn set_enabled(&self, provider_id: Uuid, model_id: &str, enabled: bool) -> StoreResult<()> {
        let mut rows = self.rows.lock().unwrap();
        match rows
            .iter_mut()
            .find(|row| row.provider_id == provider_id && row.setting.model_id == model_id)
        {
            Some(row) => row.setting.enabled = enabled,
            None => rows.push(ModelSettingRow {
                provider_id,
                setting: ModelSetting {
                    model_id: model_id.to_string(),
                    enabled,
                    is_default: false,
                },
            }),
        }
        Ok(())
    }

    async fn set_default(&self, provider_id: Uuid, model_id: &str) -> StoreResult<()> {
        let mut rows = self.rows.lock().unwrap();
        for row in rows.iter_mut().filter(|row| row.provider_id == provider_id) {
            row.setting.is_default = row.setting.model_id == model_id;
            if row.setting.model_id == model_id {
                row.setting.enabled = true;
            }
        }
        if !rows
            .iter()
            .any(|row| row.provider_id == provider_id && row.setting.model_id == model_id)
        {
            rows.push(ModelSettingRow {
                provider_id,
                setting: ModelSetting {
                    model_id: model_id.to_string(),
                    enabled: true,
                    is_default: true,
                },
            });
        }
        Ok(())
    }

    async fn is_disabled(&self, provider_id: Option<Uuid>, model_id: &str) -> StoreResult<bool> {
        let Some(provider_id) = provider_id else {
            return Ok(false);
        };
        Ok(self.rows.lock().unwrap().iter().any(|row| {
            row.provider_id == provider_id && row.setting.model_id == model_id && !row.setting.enabled
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn settings_are_provider_scoped_and_defaulting_clears_the_provider() {
        let provider = Uuid::new_v4();
        let other = Uuid::new_v4();
        let store = InMemoryModelStore::default();

        store.set_enabled(provider, "deepseek-flash", false).await.unwrap();
        assert!(store.is_disabled(Some(provider), "deepseek-flash").await.unwrap());
        assert!(!store.is_disabled(Some(other), "deepseek-flash").await.unwrap(), "other provider untouched");
        assert!(!store.is_disabled(None, "deepseek-flash").await.unwrap(), "shared key unrestricted");
        // An unconfigured model is not disabled (unrestricted).
        assert!(!store.is_disabled(Some(provider), "deepseek-v4-pro").await.unwrap());

        store.set_default(provider, "deepseek-chat").await.unwrap();
        store.set_default(provider, "deepseek-v4-pro").await.unwrap();
        let settings = store.settings(provider).await.unwrap();
        let defaults: Vec<_> = settings.iter().filter(|s| s.is_default).collect();
        assert_eq!(defaults.len(), 1, "one default per provider");
        assert_eq!(defaults[0].model_id, "deepseek-v4-pro");
    }
}
