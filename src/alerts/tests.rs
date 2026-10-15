    use super::*;

    fn th(mult: &str, floor: i64, waste: i64) -> AlertThresholds {
        AlertThresholds {
            spend_multiplier: Decimal::from_str_exact(mult).unwrap(),
            spend_floor_cents: floor,
            waste_daily_threshold: waste,
            ..AlertThresholds::defaults()
        }
    }

    #[test]
    fn limit_classes_are_entity_scoped() {
        let id = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        assert_eq!(limit_class(KEY_SPEND_LIMIT, id), format!("key_spend_limit:{id}"));
        assert_ne!(
            limit_class(KEY_SPEND_LIMIT, id),
            limit_class(PROVIDER_SPEND_LIMIT, id)
        );
    }

    #[test]
    fn defaults_are_conservative_and_whole() {
        let d = AlertThresholds::defaults();
        assert_eq!(d.spend_multiplier, Decimal::new(5, 0));
        assert_eq!(d.spend_floor_cents, 100);
        assert_eq!(d.waste_daily_threshold, 50);
    }

    #[test]
    fn spend_threshold_is_max_of_multiplier_baseline_and_floor() {
        let t = th("5", 100, 50);
        // baseline $1/hr (100c) × 5 = 500c > floor 100c -> threshold 500c
        assert_eq!(
            SpendSignal { window_cents: 0, baseline_per_hour_cents: 100 }.threshold_cents(&t),
            500
        );
        // tiny baseline $0.01/hr (1c) × 5 = 5c < floor -> floor wins (100c)
        assert_eq!(
            SpendSignal { window_cents: 0, baseline_per_hour_cents: 1 }.threshold_cents(&t),
            100
        );
        // no baseline at all -> floor applies (a new tenant can still trip the floor)
        assert_eq!(
            SpendSignal { window_cents: 0, baseline_per_hour_cents: 0 }.threshold_cents(&t),
            100
        );
    }

    #[test]
    fn spend_anomaly_strictly_exceeds_threshold() {
        let t = th("5", 100, 50);
        let signal = SpendSignal { window_cents: 500, baseline_per_hour_cents: 100 };
        assert!(!signal.is_anomalous(&t), "equal to threshold is not anomalous");
        let over = SpendSignal { window_cents: 501, baseline_per_hour_cents: 100 };
        assert!(over.is_anomalous(&t), "one cent over threshold fires");
    }

    #[test]
    fn spend_floor_catches_runaway_without_baseline() {
        let t = th("5", 100, 50);
        // A brand-new tenant with no history burns $2 in the window: 200c > floor 100c.
        let runaway = SpendSignal { window_cents: 200, baseline_per_hour_cents: 0 };
        assert!(runaway.is_anomalous(&t));
        // Normal dev traffic stays under the $1/hr floor.
        let dev = SpendSignal { window_cents: 3, baseline_per_hour_cents: 2 };
        assert!(!dev.is_anomalous(&t));
    }

    #[test]
    fn fraction_multiplier_rounds_threshold_up() {
        // multiplier 1.5 × 3c baseline = 4.5c -> ceil 5c
        let t = th("1.5", 0, 50);
        assert_eq!(
            SpendSignal { window_cents: 0, baseline_per_hour_cents: 3 }.threshold_cents(&t),
            5
        );
        // 4c window still under the 5c threshold; 5c is not > 5c either.
        let s = SpendSignal { window_cents: 5, baseline_per_hour_cents: 3 };
        assert!(!s.is_anomalous(&t));
    }

    #[test]
    fn waste_spike_passes_daily_threshold() {
        let t = th("5", 100, 50);
        assert!(!WasteSignal { flagged_requests_24h: 50 }.is_anomalous(&t));
        assert!(WasteSignal { flagged_requests_24h: 51 }.is_anomalous(&t));
        // A different per-tenant threshold applies.
        let strict = th("5", 100, 10);
        assert!(WasteSignal { flagged_requests_24h: 11 }.is_anomalous(&strict));
    }

    #[test]
    fn error_rate_fires_only_past_the_threshold_with_enough_samples() {
        let t = AlertThresholds::defaults(); // 50% over 20+ requests
        // Below the sample floor: a 100% rate still does not fire.
        assert!(!ErrorSignal { requests_24h: 19, errors_24h: 19 }.is_anomalous(&t));
        // Exactly at the threshold rate is not "past" it.
        assert!(!ErrorSignal { requests_24h: 100, errors_24h: 50 }.is_anomalous(&t));
        // Past it, with enough samples: fires.
        assert!(ErrorSignal { requests_24h: 100, errors_24h: 51 }.is_anomalous(&t));
        // A zero-traffic window can never fire.
        assert!(!ErrorSignal { requests_24h: 0, errors_24h: 0 }.is_anomalous(&t));
    }

    #[test]
    fn latency_regression_needs_a_baseline_and_a_multiple() {
        let t = AlertThresholds::defaults(); // 3x the preceding day's p95
        assert!(
            !LatencySignal { p95_ms: 500.0, baseline_p95_ms: 0.0 }.is_anomalous(&t),
            "no baseline -> nothing to regress from"
        );
        assert!(
            !LatencySignal { p95_ms: 250.0, baseline_p95_ms: 100.0 }.is_anomalous(&t),
            "2.5x is under the 3x bar"
        );
        assert!(
            LatencySignal { p95_ms: 350.0, baseline_p95_ms: 100.0 }.is_anomalous(&t),
            "3.5x regresses"
        );
    }

    #[test]
    fn class_entity_parses_per_entity_classes() {
        let id = uuid::Uuid::new_v4();
        assert_eq!(
            class_entity(&format!("provider_error:{id}"), PROVIDER_ERROR),
            Some(id)
        );
        // A different kind, a bare class, or a malformed id all yield None.
        assert_eq!(class_entity(&format!("provider_latency:{id}"), PROVIDER_ERROR), None);
        assert_eq!(class_entity("spend_velocity", PROVIDER_ERROR), None);
        assert_eq!(class_entity("provider_error:not-a-uuid", PROVIDER_ERROR), None);
    }

    #[test]
    fn lifecycle_start_fires_a_new_episode() {
        assert_eq!(
            decide_fire(false, None, 1_000, true, REFIRE_AFTER_MS),
            FireAction::Start
        );
    }

    #[test]
    fn lifecycle_silent_until_24h_elapse_while_firing() {
        // Opened at t=0, delivered at t=0. Evaluations at +1h and +23h stay silent.
        assert_eq!(
            decide_fire(true, Some(0), 3_600_000, true, REFIRE_AFTER_MS),
            FireAction::KeepSilent
        );
        assert_eq!(
            decide_fire(true, Some(0), 23 * 3_600_000, true, REFIRE_AFTER_MS),
            FireAction::KeepSilent
        );
    }

    #[test]
    fn lifecycle_refires_after_24h_while_still_firing() {
        assert_eq!(
            decide_fire(true, Some(0), 24 * 3_600_000 + 1, true, REFIRE_AFTER_MS),
            FireAction::ReFire
        );
    }

    #[test]
    fn lifecycle_clear_records_episode_end() {
        assert_eq!(
            decide_fire(true, Some(0), 3_600_000, false, REFIRE_AFTER_MS),
            FireAction::Clear
        );
    }

    #[test]
    fn lifecycle_idle_when_nothing_open_and_condition_false() {
        assert_eq!(
            decide_fire(false, None, 0, false, REFIRE_AFTER_MS),
            FireAction::Idle
        );
    }
