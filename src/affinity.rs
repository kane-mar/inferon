//! Cache-affinity routing (ITEM-063).
//!
//! Provider prompt caches are prefix-based and per provider/model, so a routing
//! decision that switches target can destroy a warm cache — paying full price
//! for tokens that were cheap a moment ago. This module keeps a per-tenant,
//! per-prefix "warm target" (the provider/model that recently served the prefix
//! from cache) and decides when to keep it instead of a marginally cheaper miss.

use crate::clock::now_secs_i64;
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

pub type StoreResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// How long a warm target stays actionable. DeepSeek's prefix cache lives for
/// minutes–hours; a stale pin must not block failover or healthy distribution.
pub const AFFINITY_TTL_SECS: i64 = 30 * 60;

/// The provider/model that most recently served a prefix from cache.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WarmTarget {
    pub provider_id: Option<Uuid>,
    pub model: String,
    /// Wall-clock seconds of the last observed cache hit.
    pub last_hit_at: i64,
}

/// Should routing keep the warm model instead of the router's cheaper choice?
///
/// Keep when the warm model's **cache-hit** input price is no worse than the
/// chosen model's **uncached** price — i.e. trading the warm cache for the
/// cheaper tier would not actually be cheaper per prompt token.
pub fn keep_warm(warm_model: &str, chosen_model: &str) -> bool {
    let warm = crate::cost::price_for(warm_model);
    let chosen = crate::cost::price_for(chosen_model);
    let warm_hit = warm.prompt_hit.unwrap_or(warm.prompt_miss);
    warm_hit <= chosen.prompt_miss
}

/// Whether a target recorded at `last_hit_at` is still within the TTL.
pub fn is_fresh(last_hit_at: i64, now_secs: i64) -> bool {
    now_secs.saturating_sub(last_hit_at) < AFFINITY_TTL_SECS
}

#[async_trait::async_trait]
pub trait AffinityStore: Send + Sync {
    /// The fresh warm target for a prefix, if any (already TTL-filtered).
    async fn warm_target(&self, tenant_id: Uuid, system_prompt_hash: &str) -> StoreResult<Option<WarmTarget>>;
    /// Record a cache hit: (re)pin the prefix's warm target.
    async fn record_hit(
        &self,
        tenant_id: Uuid,
        system_prompt_hash: &str,
        provider_id: Option<Uuid>,
        model: &str,
    ) -> StoreResult<()>;
}

/// The test/in-process double: one warm target per (tenant, prefix).
#[derive(Default)]
pub struct InMemoryAffinityStore {
    rows: Mutex<HashMap<(Uuid, String), WarmTarget>>,
    pub recorded: Mutex<usize>,
}

impl InMemoryAffinityStore {
    pub fn with_target(
        tenant_id: Uuid,
        system_prompt_hash: &str,
        model: &str,
        last_hit_at: i64,
    ) -> Self {
        let store = Self::default();
        store.rows.lock().unwrap().insert(
            (tenant_id, system_prompt_hash.to_string()),
            WarmTarget {
                provider_id: None,
                model: model.to_string(),
                last_hit_at,
            },
        );
        store
    }
}

#[async_trait::async_trait]
impl AffinityStore for InMemoryAffinityStore {
    async fn warm_target(
        &self,
        tenant_id: Uuid,
        system_prompt_hash: &str,
    ) -> StoreResult<Option<WarmTarget>> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .get(&(tenant_id, system_prompt_hash.to_string()))
            .filter(|target| is_fresh(target.last_hit_at, now_secs_i64()))
            .cloned())
    }

    async fn record_hit(
        &self,
        tenant_id: Uuid,
        system_prompt_hash: &str,
        provider_id: Option<Uuid>,
        model: &str,
    ) -> StoreResult<()> {
        *self.recorded.lock().unwrap() += 1;
        self.rows.lock().unwrap().insert(
            (tenant_id, system_prompt_hash.to_string()),
            WarmTarget {
                provider_id,
                model: model.to_string(),
                last_hit_at: now_secs_i64(),
            },
        );
        Ok(())
    }
}

/// Postgres-backed affinity (the production store).
pub struct PostgresAffinityStore {
    pool: sqlx::PgPool,
}

impl PostgresAffinityStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl AffinityStore for PostgresAffinityStore {
    async fn warm_target(
        &self,
        tenant_id: Uuid,
        system_prompt_hash: &str,
    ) -> StoreResult<Option<WarmTarget>> {
        let row: Option<(Option<Uuid>, String, i64)> = sqlx::query_as(
            "SELECT provider_id, model, EXTRACT(EPOCH FROM last_hit_at)::bigint
             FROM prompt_affinity
             WHERE tenant_id = $1 AND system_prompt_hash = $2
               AND last_hit_at > NOW() - ($3 * INTERVAL '1 second')",
        )
        .bind(tenant_id)
        .bind(system_prompt_hash)
        .bind(AFFINITY_TTL_SECS)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(provider_id, model, last_hit_at)| WarmTarget {
            provider_id,
            model,
            last_hit_at,
        }))
    }

    async fn record_hit(
        &self,
        tenant_id: Uuid,
        system_prompt_hash: &str,
        provider_id: Option<Uuid>,
        model: &str,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO prompt_affinity (tenant_id, system_prompt_hash, provider_id, model, last_hit_at)
             VALUES ($1, $2, $3, $4, NOW())
             ON CONFLICT (tenant_id, system_prompt_hash) DO UPDATE
               SET provider_id = EXCLUDED.provider_id, model = EXCLUDED.model, last_hit_at = NOW()",
        )
        .bind(tenant_id)
        .bind(system_prompt_hash)
        .bind(provider_id)
        .bind(model)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_warm_prefers_the_cached_price_when_it_beats_a_miss() {
        // A warm deepseek-flash cache (hit 0.007) beats a v4-pro miss (0.66):
        // keep the cache.
        assert!(keep_warm("deepseek-flash", "deepseek-v4-pro"));
        // A warm v4-pro cache (hit 0.022) beats a flash miss (0.22): keep it.
        assert!(keep_warm("deepseek-v4-pro", "deepseek-flash"));
        // A warm gpt-4o request has no cache-hit price (miss 2.50), which is
        // worse than gpt-4o-mini's miss (0.15): the downgrade wins.
        assert!(!keep_warm("gpt-4o", "gpt-4o-mini"));
    }

    #[test]
    fn freshness_is_bounded_by_the_ttl() {
        let now = 1_000_000;
        assert!(is_fresh(now - AFFINITY_TTL_SECS + 1, now));
        assert!(!is_fresh(now - AFFINITY_TTL_SECS, now));
        assert!(!is_fresh(now - AFFINITY_TTL_SECS - 1, now));
    }

    #[tokio::test]
    async fn a_recorded_target_is_returned_and_ages_out() {
        let tenant = Uuid::new_v4();
        let store = InMemoryAffinityStore::default();
        assert_eq!(store.warm_target(tenant, "h").await.unwrap(), None);
        store.record_hit(tenant, "h", None, "deepseek-flash").await.unwrap();
        let target = store.warm_target(tenant, "h").await.unwrap().expect("warm");
        assert_eq!(target.model, "deepseek-flash");
        assert_eq!(*store.recorded.lock().unwrap(), 1);
        // An expired pin is not returned.
        let stale = InMemoryAffinityStore::with_target(tenant, "old", "m", now_secs_i64() - AFFINITY_TTL_SECS - 1);
        assert_eq!(stale.warm_target(tenant, "old").await.unwrap(), None);
    }
}
