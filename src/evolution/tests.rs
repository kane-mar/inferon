// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

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

    use std::sync::atomic::{AtomicBool, Ordering};

    struct TrainerDouble {
        outcome: Result<Candidate, String>,
    }

    #[async_trait::async_trait]
    impl Trainer for TrainerDouble {
        async fn train(&self, _job: &EvolutionJob) -> Result<Candidate, String> {
            self.outcome.clone()
        }
    }

    struct GateDouble {
        outcome: Result<bool, String>,
    }

    #[async_trait::async_trait]
    impl Gate for GateDouble {
        async fn passes(&self, _job: &EvolutionJob, _candidate: &Candidate) -> Result<bool, String> {
            self.outcome.clone()
        }
    }

    struct PromoterDouble {
        outcome: Result<(), String>,
        called: AtomicBool,
    }

    impl PromoterDouble {
        fn new(outcome: Result<(), String>) -> Self {
            Self {
                outcome,
                called: AtomicBool::new(false),
            }
        }

        fn called(&self) -> bool {
            self.called.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl Promoter for PromoterDouble {
        async fn promote(&self, _job: &EvolutionJob, _candidate: &Candidate) -> Result<(), String> {
            self.called.store(true, Ordering::SeqCst);
            self.outcome.clone()
        }
    }

    fn candidate() -> Candidate {
        Candidate {
            adapter: "local-adapter-v2".to_string(),
        }
    }

    async fn store_with_job() -> (InMemoryEvolutionStore, Uuid) {
        let store = InMemoryEvolutionStore::default();
        let tenant = Uuid::new_v4();
        assert!(store
            .enqueue(tenant, "deepseek-flash", &json!({"reason": "drift"}))
            .await
            .unwrap());
        (store, tenant)
    }

    #[tokio::test]
    async fn a_queued_job_runs_train_gate_promote_and_records_the_state() {
        let (store, tenant) = store_with_job().await;
        let trainer = TrainerDouble {
            outcome: Ok(candidate()),
        };
        let gate = GateDouble { outcome: Ok(true) };
        let promoter = PromoterDouble::new(Ok(()));

        let outcome = process_one(&store, &trainer, &gate, &promoter).await.unwrap();

        assert_eq!(outcome, Some(JobState::Promoted));
        assert!(promoter.called());
        let job = &store.list(tenant).await.unwrap()[0];
        assert_eq!(job.state, "promoted");
        assert!(job.finished_at.is_some(), "terminal state is stamped");
        assert_eq!(job.evidence["adapter"], "local-adapter-v2");
    }

    #[tokio::test]
    async fn a_training_failure_records_the_stage_and_never_promotes() {
        let (store, tenant) = store_with_job().await;
        let trainer = TrainerDouble {
            outcome: Err("no GPU".to_string()),
        };
        let gate = GateDouble { outcome: Ok(true) };
        let promoter = PromoterDouble::new(Ok(()));

        let outcome = process_one(&store, &trainer, &gate, &promoter).await.unwrap();

        assert_eq!(outcome, Some(JobState::Failed));
        assert!(!promoter.called());
        let job = &store.list(tenant).await.unwrap()[0];
        assert_eq!(job.state, "failed");
        assert_eq!(job.evidence["stage"], "training");
        assert_eq!(job.evidence["error"], "no GPU");
        assert!(job.finished_at.is_some());
    }

    #[tokio::test]
    async fn a_rejected_candidate_is_never_promoted() {
        let (store, tenant) = store_with_job().await;
        let trainer = TrainerDouble {
            outcome: Ok(candidate()),
        };
        let gate = GateDouble { outcome: Ok(false) };
        let promoter = PromoterDouble::new(Ok(()));

        let outcome = process_one(&store, &trainer, &gate, &promoter).await.unwrap();

        assert_eq!(outcome, Some(JobState::Failed));
        assert!(!promoter.called(), "the gold set runs before promotion");
        let job = &store.list(tenant).await.unwrap()[0];
        assert_eq!(job.evidence["stage"], "gate");
        assert_eq!(job.state, "failed");
    }

    #[tokio::test]
    async fn a_promotion_failure_is_recorded() {
        let (store, tenant) = store_with_job().await;
        let trainer = TrainerDouble {
            outcome: Ok(candidate()),
        };
        let gate = GateDouble { outcome: Ok(true) };
        let promoter = PromoterDouble::new(Err("load_lora_adapter failed".to_string()));

        let outcome = process_one(&store, &trainer, &gate, &promoter).await.unwrap();

        assert_eq!(outcome, Some(JobState::Failed));
        let job = &store.list(tenant).await.unwrap()[0];
        assert_eq!(job.evidence["stage"], "promoting");
        assert_eq!(job.evidence["error"], "load_lora_adapter failed");
    }

    #[tokio::test]
    async fn an_idle_queue_does_nothing() {
        let store = InMemoryEvolutionStore::default();
        let trainer = TrainerDouble {
            outcome: Ok(candidate()),
        };
        let gate = GateDouble { outcome: Ok(true) };
        let promoter = PromoterDouble::new(Ok(()));

        let outcome = process_one(&store, &trainer, &gate, &promoter).await.unwrap();

        assert_eq!(outcome, None);
        assert!(!promoter.called());
    }

    #[tokio::test]
    async fn each_job_is_claimed_once() {
        let store = InMemoryEvolutionStore::default();
        let tenant = Uuid::new_v4();
        store.enqueue(tenant, "a", &json!({})).await.unwrap();
        store.enqueue(tenant, "b", &json!({})).await.unwrap();

        let first = store.claim().await.unwrap().unwrap();
        let second = store.claim().await.unwrap().unwrap();

        assert_ne!(first.id, second.id);
        assert_eq!(first.state, "training");
        assert_eq!(second.state, "training");
        assert!(store.claim().await.unwrap().is_none(), "both are taken");
    }
