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
        assert_eq!(
            recommend_threshold(0.35, Objective::Balanced, MIN_SAMPLE - 1, 5, Some(0.9)),
            0.35
        );
        assert_eq!(recommend_threshold(0.35, Objective::Balanced, 100, 90, None), 0.35);
    }

    #[test]
    fn a_worse_cheap_tier_raises_the_threshold() {
        // 60% success on downgraded vs 90% baseline -> downgrade less.
        assert!(approx(
            recommend_threshold(0.35, Objective::Balanced, 100, 60, Some(0.9)),
            0.40
        ));
    }

    #[test]
    fn an_every_bit_as_good_cheap_tier_lowers_the_threshold() {
        // 92% on downgraded vs 90% baseline -> downgrade more.
        assert!(approx(
            recommend_threshold(0.35, Objective::Balanced, 100, 92, Some(0.9)),
            0.30
        ));
    }

    #[test]
    fn the_step_is_bounded_at_the_limits() {
        assert_eq!(recommend_threshold(0.0, Objective::Balanced, 100, 99, Some(0.9)), 0.15);
        assert_eq!(recommend_threshold(0.9, Objective::Balanced, 100, 10, Some(0.9)), 0.9);
    }

    #[test]
    fn quality_first_only_downgrades_when_the_cheap_tier_is_clearly_better() {
        // ITEM-141: the workspace said quality matters more than price. Equal
        // quality is not a reason to downgrade; clearly better is.
        assert!(approx(
            recommend_threshold(0.60, Objective::Quality, 100, 90, Some(0.90)),
            0.60
        ));
        assert!(approx(
            recommend_threshold(0.60, Objective::Quality, 100, 98, Some(0.90)),
            0.55
        ));
    }

    #[test]
    fn quality_first_moves_up_twice_as_fast_on_a_shortfall() {
        // The thing quality mode exists to prevent, so it acts decisively.
        assert!(approx(
            recommend_threshold(0.60, Objective::Quality, 100, 55, Some(0.90)),
            0.70
        ));
        // The same evidence under balanced is one step.
        assert!(approx(
            recommend_threshold(0.60, Objective::Balanced, 100, 55, Some(0.90)),
            0.65
        ));
    }

    #[test]
    fn each_objective_has_a_floor_the_loop_cannot_cross() {
        // A live finding: repeated steps once took a workspace's threshold to
        // 1.4e-17 — "always downgrade" — with nobody able to see it.
        // One step below each floor lands exactly on it; a second attempt stays.
        assert_eq!(recommend_threshold(0.06, Objective::Cost, 100, 100, Some(0.5)), 0.05);
        assert_eq!(recommend_threshold(0.05, Objective::Cost, 100, 100, Some(0.5)), 0.05);
        assert_eq!(recommend_threshold(0.16, Objective::Balanced, 100, 100, Some(0.5)), 0.15);
        assert_eq!(recommend_threshold(0.51, Objective::Quality, 100, 100, Some(0.5)), 0.50);
        assert_eq!(Objective::Cost.floor(), 0.05);
        assert_eq!(Objective::Balanced.floor(), 0.15);
        assert_eq!(Objective::Quality.floor(), 0.50);
    }

    #[test]
    fn cost_first_tolerates_a_small_quality_cost() {
        // Within the margin of the baseline is still "good enough" at the prices
        // this objective cares about; balanced would hold still.
        assert!(approx(
            recommend_threshold(0.35, Objective::Cost, 100, 88, Some(0.90)),
            0.30
        ));
        assert!(approx(
            recommend_threshold(0.35, Objective::Balanced, 100, 88, Some(0.90)),
            0.35
        ));
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
            .publish(tenant, Policy { version: 0, prune: None, router_threshold: Some(0.30), evidence: json!({"why": "v1"}), enabled: true, objective: Objective::Balanced, provider_weights: Default::default() })
            .await
            .unwrap();
        assert_eq!(v1.version, 1);
        let v2 = store
            .publish(tenant, Policy { version: 0, prune: None, router_threshold: Some(0.35), evidence: json!({"why": "v2"}), enabled: true, objective: Objective::Balanced, provider_weights: Default::default() })
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
        objective: Objective::Balanced,
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
        objective: Objective::Balanced,
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

#[test]
fn prompt_waste_has_to_be_worth_the_interruption() {
    // ITEM-141: enabling pruning changes answers, so the loop only proposes it
    // when the priced waste is real money and a real share of spend.
    let plan = |usd: &str| PromptPlan {
        waste_usd: usd.parse().unwrap(),
        requests: 40,
        top_kind: Some("repeated_system_prompt".to_string()),
    };

    assert!(!plan("0.40").worth_pruning("10.00".parse().unwrap()), "cents are not worth a proposal");
    assert!(!plan("5.00").worth_pruning("200.00".parse().unwrap()), "2.5% of spend is not a waste problem");
    assert!(plan("5.00").worth_pruning("40.00".parse().unwrap()), "12.5% of spend is");
    assert!(plan("5.00").worth_pruning(rust_decimal::Decimal::ZERO), "no spend yet: price alone decides");
}

// ITEM-147: the rules an operator's change has to satisfy, as a pure function.
// They were only reachable through an HTTP handler, so the boundary cases were
// tested as side effects of a request.

fn operator_change(threshold: Option<f64>, objective: Option<&str>, weights: Option<(&str, f64)>) -> OperatorChange {
    OperatorChange {
        enabled: None,
        router_threshold: threshold,
        objective: objective.map(str::to_string),
        provider_weights: weights.map(|(id, w)| [(id.to_string(), w)].into_iter().collect()),
    }
}

#[test]
fn a_threshold_outside_the_policy_bounds_is_refused() {
    // Refused, never clamped: a number the operator did not type is a number they
    // cannot debug.
    assert_eq!(
        operator_change(Some(0.95), None, None).apply(None),
        Err(PolicyError::InvalidThreshold)
    );
    assert_eq!(
        operator_change(Some(-0.1), None, None).apply(None),
        Err(PolicyError::InvalidThreshold)
    );
    assert!(operator_change(Some(MAX_THRESHOLD), None, None).apply(None).is_ok());
}

#[test]
fn a_weight_outside_zero_to_one_is_refused() {
    assert_eq!(
        operator_change(None, None, Some(("p1", 1.5))).apply(None),
        Err(PolicyError::InvalidWeight)
    );
    assert_eq!(
        operator_change(None, None, Some(("p1", -0.1))).apply(None),
        Err(PolicyError::InvalidWeight)
    );
    assert!(operator_change(None, None, Some(("p1", 0.6))).apply(None).is_ok());
}

#[test]
fn an_objective_the_loop_does_not_know_is_refused_by_name() {
    assert_eq!(
        operator_change(None, Some("fastest"), None).apply(None),
        Err(PolicyError::UnknownObjective("fastest".to_string()))
    );
}

#[test]
fn a_partial_change_keeps_everything_it_did_not_name() {
    let current = Policy {
        version: 7,
        router_threshold: Some(0.3),
        provider_weights: [("p1".to_string(), 0.5)].into_iter().collect(),
        prune: Some(crate::prune::PrunePolicy { enabled: true, ..Default::default() }),
        evidence: serde_json::json!({"reason": "adaptive_step"}),
        objective: Objective::Quality,
        enabled: true,
    };

    let next = operator_change(Some(0.4), None, None)
        .apply(Some(&current))
        .expect("a valid change");

    assert_eq!(next.router_threshold, Some(0.4), "the named field changed");
    assert_eq!(next.objective, Objective::Quality, "the objective was not named");
    assert_eq!(next.provider_weights, current.provider_weights);
    assert!(next.prune.is_some(), "pruning survives a threshold edit");
}

#[test]
fn the_operator_steer_records_itself_and_keeps_the_loops_observation() {
    // A human changing the objective must not erase the prompt-waste evidence.
    let current = Policy {
        version: 1,
        router_threshold: Some(0.3),
        provider_weights: Default::default(),
        prune: None,
        evidence: serde_json::json!({"prompt": {"waste_usd": "2.50", "propose_pruning": true}}),
        objective: Objective::Balanced,
        enabled: true,
    };

    let next = operator_change(None, Some("cost"), None)
        .apply(Some(&current))
        .expect("a valid change");

    assert_eq!(next.evidence["source"], serde_json::json!("operator"));
    assert_eq!(next.evidence["prompt"]["waste_usd"], serde_json::json!("2.50"));
    assert_eq!(next.objective, Objective::Cost);
}

#[test]
fn the_first_steer_on_an_unconfigured_workspace_is_a_valid_policy() {
    let next = operator_change(Some(0.35), Some("balanced"), None)
        .apply(None)
        .expect("no current policy is not an error");

    assert_eq!(next.router_threshold, Some(0.35));
    assert_eq!(next.objective, Objective::Balanced);
    assert!(next.enabled);
}
