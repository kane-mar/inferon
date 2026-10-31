    use super::*;
    use crate::quality::Outcome;
    use rust_decimal::Decimal;

    fn trace(model: &str, tokens: i32) -> TraceMessage {
        TraceMessage {
            id: Uuid::new_v4(),
            tenant_id: Uuid::new_v4(),
            request_model: model.to_string(),
            system_prompt_hash: None,
            prompt_tokens: tokens,
            completion_tokens: 1,
            cached_prompt_tokens: None,
            ttft_ms: Some(5),
            total_duration_ms: 10,
            status_code: 200,
            raw_prompt: None,
            raw_completion: None,
            raw_completion_capped: false,
            waste_detected: None,
            upstream_cost_usd: Decimal::ZERO,
            cache_savings_usd: Decimal::ZERO,
            truncated: false,
            guard_event: None,
            routing_event: None,
            router_event: None,
            provider_id: None,
            is_local: false,
            api_key_id: None,
            outcome: Outcome::Ok,
            error_class: None,
            savings_usd: Decimal::ZERO,
            savings_kind: None,
            cache_affinity: None,
            policy_version: None,
            proxy_ms: None,
                proxy_us: None,
                example_state: None,
                example_reason: None,
                completion_hash: None,
                session_id: None,
                request_lint: None,
        }
    }

    #[test]
    fn wire_trace_is_the_cloud_edge_shape() {
        let value = wire_trace(&trace("local-llm", 7));
        assert_eq!(value["model"], "local-llm");
        assert_eq!(value["prompt_tokens"], 7);
        assert_eq!(value["outcome"], "ok");
        assert!(value["error_class"].is_null());
    }

    #[tokio::test]
    async fn edge_state_authenticates_synced_hashes_and_replaces_them() {
        let tenant = Uuid::new_v4();
        let state = EdgeState::new(EdgeConfig {
            tenant_id: Some(tenant),
            key_hashes: vec!["a".to_string()],
            ..Default::default()
        });
        assert!(state.tenant_by_api_key_hash("a").await.unwrap().is_some());
        assert!(state.tenant_by_api_key_hash("b").await.unwrap().is_none());
        // A resync replaces the set (revoked keys stop authenticating).
        state.replace(EdgeConfig {
            tenant_id: Some(tenant),
            key_hashes: vec!["b".to_string()],
            ..Default::default()
        });
        assert!(state.tenant_by_api_key_hash("a").await.unwrap().is_none());
        assert!(state.tenant_by_api_key_hash("b").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn edge_state_exposes_the_synced_budget_routing_and_policy() {
        let state = EdgeState::new(EdgeConfig {
            tenant_id: Some(Uuid::new_v4()),
            fallbacks: vec!["local-small".to_string()],
            budget: Some(EdgeBudget {
                limit_cents: 500,
                guard_enabled: false,
                ..Default::default()
            }),
            ..Default::default()
        });
        let budget = state.budget_for(Uuid::new_v4()).await.unwrap().unwrap();
        assert_eq!(budget.limit_cents, 500);
        assert!(!state.guard_enabled_for(Uuid::new_v4()).await.unwrap());
        assert_eq!(state.fallbacks(Uuid::new_v4()).await.unwrap(), vec!["local-small"]);
        assert!(state.active(Uuid::new_v4()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn remote_sink_keeps_traces_when_the_cloud_is_unreachable() {
        // Port 1 is never listening: the batch must be retained, not lost.
        let mut sink = RemoteTraceSink::new("http://127.0.0.1:1", "token");
        let mut batch = vec![trace("m", 1), trace("m", 2)];
        sink.flush(&mut batch).await;
        assert!(batch.is_empty());
        assert_eq!(sink.pending.len(), 2, "unsent traces are buffered");
    }

    fn options(provider_url: &str) -> EdgeOptions {
        EdgeOptions {
            control_url: String::new(),
            node_token: String::new(),
            provider_url: provider_url.to_string(),
            provider_key: "sk-local".to_string(),
            provider_kind: "openai-compatible".to_string(),
            port: 0,
            sync_interval: std::time::Duration::from_secs(1),
        }
    }

    #[tokio::test]
    async fn discovers_the_local_providers_models_and_falls_back() {
        // A local server advertising its own model ids.
        let host = axum::Router::new().route(
            "/v1/models",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!({"data":[{"id":"qwen2.5-7b-instruct"}]}))
            }),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, host).await.unwrap() });

        let cloud = vec!["deepseek-flash".to_string()];
        let client = reqwest::Client::new();
        assert_eq!(
            local_models(&client, &options(&format!("http://{addr}/v1")), &cloud).await,
            vec!["qwen2.5-7b-instruct"],
            "the node serves what the local server serves"
        );
        // No local server -> the cloud catalog is the fallback, not a failure.
        assert_eq!(
            local_models(&client, &options("http://127.0.0.1:1/v1"), &cloud).await,
            cloud
        );
    }

    #[test]
    fn a_credential_can_come_from_a_file_so_it_never_appears_in_argv() {
        assert_eq!(
            credential(Some("sk-argv".into()), None, "--provider-key").unwrap(),
            "sk-argv"
        );
        let path = std::env::temp_dir().join(format!("inferon-key-{}", Uuid::new_v4()));
        std::fs::write(&path, "sk-from-file\n").unwrap();
        assert_eq!(
            credential(
                Some("sk-argv".into()),
                Some(path.to_string_lossy().into_owned()),
                "--provider-key",
            )
            .unwrap(),
            "sk-from-file",
            "the file wins, so a stale inline flag cannot shadow it"
        );
        assert!(credential(
            None,
            Some("/nonexistent/inferon-key".into()),
            "--provider-key"
        )
        .is_err());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_missing_credential_names_both_forms() {
        let error = credential(None, None, "--provider-key").unwrap_err();
        assert!(error.contains("--provider-key"), "got {error}");
        assert!(error.contains("--provider-key-file"), "got {error}");
    }
