// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

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
