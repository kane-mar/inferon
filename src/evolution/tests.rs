    use super::*;

    #[test]
    fn a_fallback_rate_is_drift_only_above_the_threshold_with_enough_evidence() {
        // Exactly 5% is not *above* the threshold.
        assert!(!DriftSignal {
            adapter: "deepseek-flash".into(),
            requests: 20,
            fallbacks: 1
        }
        .is_drifted());
        // 2/21 = 9.5% is.
        assert!(DriftSignal {
            adapter: "deepseek-flash".into(),
            requests: 21,
            fallbacks: 2
        }
        .is_drifted());
        // Below the sample floor even 100% is noise, not drift.
        assert!(!DriftSignal {
            adapter: "deepseek-flash".into(),
            requests: 19,
            fallbacks: 19
        }
        .is_drifted());
        // No traffic never divides by zero.
        assert_eq!(
            DriftSignal {
                adapter: "m".into(),
                requests: 0,
                fallbacks: 0
            }
            .rate(),
            0.0
        );
    }

    #[tokio::test]
    async fn enqueue_never_duplicates_an_open_job() {
        let store = InMemoryEvolutionStore::default();
        let tenant = Uuid::new_v4();
        let evidence = json!({"reason": "drift"});

        assert!(store
            .enqueue(tenant, "deepseek-flash", &evidence)
            .await
            .unwrap());
        assert!(
            !store
                .enqueue(tenant, "deepseek-flash", &evidence)
                .await
                .unwrap(),
            "one open job per (tenant, adapter)"
        );
        assert!(
            store
                .enqueue(tenant, "deepseek-v4-pro", &evidence)
                .await
                .unwrap(),
            "a different adapter is a different job"
        );
        // Another tenant is independent.
        assert!(store
            .enqueue(Uuid::new_v4(), "deepseek-flash", &evidence)
            .await
            .unwrap());

        let jobs = store.list(tenant).await.unwrap();
        assert_eq!(jobs.len(), 2);
        assert!(jobs.iter().all(|job| job.state == "queued"));
        assert!(jobs.iter().all(|job| job.reason == "drift"));
    }
