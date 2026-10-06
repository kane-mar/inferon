//! Fallback & routing rules (ITEM-045): a tenant's ordered chain of models tried
//! when the primary request fails retryably. `RoutingStore` is the seam;
//! `RoutingPolicy` is what the proxy loads once per request (an empty chain is
//! the default and changes nothing).

use uuid::Uuid;

pub type StoreResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Failure classes that trigger the next chain step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackReason {
    Transport,
    RateLimited,
    ServerError,
}

impl FallbackReason {
    pub fn label(self) -> &'static str {
        match self {
            FallbackReason::Transport => "upstream_transport",
            FallbackReason::RateLimited => "upstream_429",
            FallbackReason::ServerError => "upstream_5xx",
        }
    }
}

/// A 429 or 5xx (or a transport error) is retryable; a 4xx that isn't 429 is a
/// request problem and is surfaced as-is. `None` = not retryable.
pub fn fallback_reason(status: u16) -> Option<FallbackReason> {
    match status {
        429 => Some(FallbackReason::RateLimited),
        code if (500..600).contains(&code) => Some(FallbackReason::ServerError),
        _ => None,
    }
}

#[async_trait::async_trait]
pub trait RoutingStore: Send + Sync {
    /// The tenant's ordered fallback models (empty = no chain).
    async fn fallbacks(&self, tenant_id: Uuid) -> StoreResult<Vec<String>>;
    async fn set_fallbacks(&self, tenant_id: Uuid, models: &[String]) -> StoreResult<()>;
}

// --- Postgres ---------------------------------------------------------------

pub struct PostgresRoutingStore {
    pool: sqlx::PgPool,
}

impl PostgresRoutingStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl RoutingStore for PostgresRoutingStore {
    async fn fallbacks(&self, tenant_id: Uuid) -> StoreResult<Vec<String>> {
        let row: Option<(sqlx::types::Json<Vec<String>>,)> =
            sqlx::query_as("SELECT fallbacks FROM routing_policies WHERE tenant_id = $1")
                .bind(tenant_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(json,)| json.0).unwrap_or_default())
    }

    async fn set_fallbacks(&self, tenant_id: Uuid, models: &[String]) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO routing_policies (tenant_id, fallbacks) VALUES ($1, $2)
             ON CONFLICT (tenant_id)
             DO UPDATE SET fallbacks = $2, updated_at = NOW()",
        )
        .bind(tenant_id)
        .bind(sqlx::types::Json(models))
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

// --- In-memory (tests) ------------------------------------------------------

#[derive(Default)]
pub struct InMemoryRoutingStore {
    rows: std::sync::Mutex<Vec<(Uuid, Vec<String>)>>,
}

impl InMemoryRoutingStore {
    pub fn with_fallbacks(tenant_id: Uuid, models: &[&str]) -> Self {
        Self {
            rows: std::sync::Mutex::new(vec![(
                tenant_id,
                models.iter().map(|m| m.to_string()).collect(),
            )]),
        }
    }
}

#[async_trait::async_trait]
impl RoutingStore for InMemoryRoutingStore {
    async fn fallbacks(&self, tenant_id: Uuid) -> StoreResult<Vec<String>> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .find(|(id, _)| *id == tenant_id)
            .map(|(_, models)| models.clone())
            .unwrap_or_default())
    }

    async fn set_fallbacks(&self, tenant_id: Uuid, models: &[String]) -> StoreResult<()> {
        let mut rows = self.rows.lock().unwrap();
        match rows.iter_mut().find(|(id, _)| *id == tenant_id) {
            Some((_, existing)) => *existing = models.to_vec(),
            None => rows.push((tenant_id, models.to_vec())),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_retryable_failures_trigger_a_fallback() {
        assert_eq!(fallback_reason(429), Some(FallbackReason::RateLimited));
        assert_eq!(fallback_reason(500), Some(FallbackReason::ServerError));
        assert_eq!(fallback_reason(503), Some(FallbackReason::ServerError));
        // A request problem is surfaced as-is, never retried on another model.
        assert_eq!(fallback_reason(400), None);
        assert_eq!(fallback_reason(401), None);
        assert_eq!(fallback_reason(404), None);
        assert_eq!(fallback_reason(200), None);
    }

    #[tokio::test]
    async fn chains_round_trip_and_are_tenant_scoped() {
        let tenant = Uuid::new_v4();
        let other = Uuid::new_v4();
        let store = InMemoryRoutingStore::default();
        assert!(store.fallbacks(tenant).await.unwrap().is_empty(), "no chain by default");

        store
            .set_fallbacks(tenant, &["gpt-4o-mini".to_string(), "deepseek-flash".to_string()])
            .await
            .unwrap();
        assert_eq!(
            store.fallbacks(tenant).await.unwrap(),
            vec!["gpt-4o-mini", "deepseek-flash"],
            "order is preserved"
        );
        assert!(store.fallbacks(other).await.unwrap().is_empty(), "other tenant untouched");
    }
}
