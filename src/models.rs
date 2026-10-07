//! Model catalog + per-provider selection (ITEM-044).
//!
//! `CATALOG` is the static list of models Inferon can serve, keyed by provider
//! kind, with the metadata a tenant needs to choose. **Prices come from
//! [`crate::cost::price_for`]** so the catalog and the cost engine can never
//! disagree — the `deepseek-flash` default-row overcharge (2026-10-05) is why.
//!
//! [`ModelStore`] holds enable/disable + default flags **per provider**: a
//! provider owns its models, and the curated catalog for the provider's kind is
//! what it can serve. Models that are not in the catalog (e.g. an
//! `openai-compatible` provider's own ids) are unrestricted — no rows needed.

use uuid::Uuid;

pub type StoreResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: &'static str,
    pub kind: &'static str,
    pub name: &'static str,
    pub context_window: u32,
    pub max_output: u32,
    pub reasoning: bool,
    pub images: bool,
}

pub const CATALOG: &[ModelInfo] = &[
    ModelInfo {
        id: "deepseek-chat",
        kind: "deepseek",
        name: "DeepSeek Chat",
        context_window: 1_000_000,
        max_output: 384_000,
        reasoning: false,
        images: false,
    },
    ModelInfo {
        id: "deepseek-flash",
        kind: "deepseek",
        name: "DeepSeek V4.1 Flash",
        context_window: 1_000_000,
        max_output: 384_000,
        reasoning: true,
        images: false,
    },
    ModelInfo {
        id: "deepseek-v4-pro",
        kind: "deepseek",
        name: "DeepSeek V4 Pro",
        context_window: 1_000_000,
        max_output: 384_000,
        reasoning: true,
        images: false,
    },
    ModelInfo {
        id: "gpt-4o",
        kind: "openai",
        name: "GPT-4o",
        context_window: 128_000,
        max_output: 16_384,
        reasoning: false,
        images: true,
    },
    ModelInfo {
        id: "gpt-4o-mini",
        kind: "openai",
        name: "GPT-4o mini",
        context_window: 128_000,
        max_output: 16_384,
        reasoning: false,
        images: true,
    },
];

pub fn find(id: &str) -> Option<&'static ModelInfo> {
    CATALOG.iter().find(|model| model.id == id)
}

/// Models of one provider kind (empty for `openai-compatible`).
pub fn by_kind(kind: &str) -> impl Iterator<Item = &'static ModelInfo> + '_ {
    CATALOG.iter().filter(move |model| model.kind == kind)
}

pub fn ids_of_kind(kind: &str) -> Vec<&'static str> {
    by_kind(kind).map(|model| model.id).collect()
}

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

    #[test]
    fn catalog_prices_come_from_the_cost_engine() {
        // The catalog has no price fields of its own — the single source is
        // cost::price_for, so the two can never disagree.
        let price = crate::cost::price_for("deepseek-flash");
        assert_eq!(price.prompt_miss.to_string(), "0.22");
        assert!(find("deepseek-flash").is_some());
    }

    #[test]
    fn catalog_lookup_helpers() {
        assert_eq!(find("gpt-4o").map(|m| m.kind), Some("openai"));
        assert!(find("no-such-model").is_none());
        assert_eq!(ids_of_kind("deepseek"), vec!["deepseek-chat", "deepseek-flash", "deepseek-v4-pro"]);
        assert_eq!(by_kind("openai-compatible").count(), 0);
    }

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
