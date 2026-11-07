// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

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
            .publish(tenant, Policy { version: 0, prune: None, router_threshold: Some(0.30), evidence: json!({"why": "v1"}), enabled: true, provider_weights: Default::default() })
            .await
            .unwrap();
        assert_eq!(v1.version, 1);
        let v2 = store
            .publish(tenant, Policy { version: 0, prune: None, router_threshold: Some(0.35), evidence: json!({"why": "v2"}), enabled: true, provider_weights: Default::default() })
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

#[test]
fn a_pruning_opt_in_keeps_the_routing_policy_and_records_the_previous_version() {
    // ITEM-091 AC6: opting into pruning must not reset routing, and the change
    // must be attributable to the version it replaced.
    let current = Policy {
        version: 4,
        router_threshold: Some(0.3),
        prune: None,
        evidence: serde_json::json!({"why": "adaptive"}),
        enabled: true,
        provider_weights: [("p1".to_string(), 0.5)].into_iter().collect(),
    };
    let next = prune_policy_from(
        Some(&current),
        crate::prune::PrunePolicy {
            enabled: true,
            drop_duplicate_turns: true,
            max_prompt_tokens: Some(4_000),
            holdout_percent: 99,
        },
    );
    assert_eq!(next.version, 4, "the store assigns the new version");
    assert_eq!(next.router_threshold, Some(0.3), "routing is untouched");
    assert_eq!(next.provider_weights, current.provider_weights);
    assert_eq!(next.evidence["previous_version"], 4);
    assert_eq!(next.prune.as_ref().unwrap().holdout_percent, 50, "holdout is clamped");
    assert!(next.prune.as_ref().unwrap().enabled);
}

#[test]
fn a_tenant_without_a_policy_can_still_opt_in() {
    let next = prune_policy_from(
        None,
        crate::prune::PrunePolicy {
            enabled: true,
            drop_duplicate_turns: true,
            max_prompt_tokens: None,
            holdout_percent: 10,
        },
    );
    assert_eq!(next.version, 0);
    assert_eq!(next.router_threshold, None, "no opinion on routing");
    assert_eq!(next.evidence["previous_version"], serde_json::Value::Null);
    assert!(next.enabled);
}

#[test]
fn opting_out_publishes_a_disabled_policy_rather_than_deleting_history() {
    let current = Policy {
        version: 2,
        router_threshold: None,
        prune: Some(crate::prune::PrunePolicy::default()),
        evidence: serde_json::json!({}),
        enabled: true,
        provider_weights: Default::default(),
    };
    let next = prune_policy_from(
        Some(&current),
        crate::prune::PrunePolicy {
            enabled: false,
            ..Default::default()
        },
    );
    assert!(!next.prune.as_ref().unwrap().enabled);
    assert_eq!(next.evidence["enabled"], false);
}
