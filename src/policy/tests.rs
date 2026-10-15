    use super::*;
    use serde_json::json;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn no_move_without_enough_evidence() {
        // Too few downgraded requests, or no baseline: keep the current value.
        assert_eq!(recommend_threshold(0.35, MIN_SAMPLE - 1, 5, Some(0.9)), 0.35);
        assert_eq!(recommend_threshold(0.35, 100, 90, None), 0.35);
    }

    #[test]
    fn a_worse_cheap_tier_raises_the_threshold() {
        // 60% success on downgraded vs 90% baseline -> downgrade less.
        assert!(approx(recommend_threshold(0.35, 100, 60, Some(0.9)), 0.40));
    }

    #[test]
    fn an_every_bit_as_good_cheap_tier_lowers_the_threshold() {
        // 92% on downgraded vs 90% baseline -> downgrade more.
        assert!(approx(recommend_threshold(0.35, 100, 92, Some(0.9)), 0.30));
    }

    #[test]
    fn the_step_is_bounded_at_the_limits() {
        assert_eq!(recommend_threshold(0.0, 100, 99, Some(0.9)), 0.0);
        assert_eq!(recommend_threshold(0.9, 100, 10, Some(0.9)), 0.9);
    }

    #[test]
    fn provider_weight_is_neutral_without_enough_data() {
        // Below the sample floor: no opinion, never a silent penalty.
        assert_eq!(provider_weight(9, 9, 5000.0, 100.0, 20), 1.0);
    }

    #[test]
    fn provider_weight_rewards_quality_and_speed() {
        // Perfect quality at the median latency -> full weight.
        assert!(approx(provider_weight(100, 0, 100.0, 100.0, 20), 1.0));
        // 50% errors and twice the median latency -> 0.5 x 0.5 = 0.25.
        assert!(approx(provider_weight(100, 50, 200.0, 100.0, 20), 0.25));
        // A healthy provider is never demoted below the floor.
        assert!(approx(provider_weight(100, 100, 100.0, 100.0, 20), 0.1));
    }

    #[tokio::test]
    async fn versions_are_append_only_and_rollback_restores() {
        let tenant = Uuid::new_v4();
        let store = InMemoryPolicyStore::default();
        assert_eq!(store.active(tenant).await.unwrap(), None);
        let v1 = store
            .publish(tenant, Policy { version: 0, router_threshold: Some(0.30), evidence: json!({"why": "v1"}), enabled: true, provider_weights: Default::default() })
            .await
            .unwrap();
        assert_eq!(v1.version, 1);
        let v2 = store
            .publish(tenant, Policy { version: 0, router_threshold: Some(0.35), evidence: json!({"why": "v2"}), enabled: true, provider_weights: Default::default() })
            .await
            .unwrap();
        assert_eq!(v2.version, 2);
        assert_eq!(store.active(tenant).await.unwrap().unwrap().version, 2);
        // Rollback restores v1's threshold as a NEW version (auditable).
        let restored = store.rollback(tenant, 1).await.unwrap().unwrap();
        assert_eq!(restored.version, 3);
        assert_eq!(restored.router_threshold, Some(0.30));
        assert_eq!(store.active(tenant).await.unwrap().unwrap().version, 3);
        // The kill switch freezes adaptation.
        store.freeze(tenant).await.unwrap();
        assert_eq!(store.active(tenant).await.unwrap(), None);
    }
