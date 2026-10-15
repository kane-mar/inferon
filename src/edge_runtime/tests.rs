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
            waste_detected: None,
            upstream_cost_usd: Decimal::ZERO,
            cache_savings_usd: Decimal::ZERO,
            truncated: false,
            guard_event: None,
            routing_event: None,
            router_event: None,
            provider_id: None,
            api_key_id: None,
            outcome: Outcome::Ok,
            error_class: None,
            savings_usd: Decimal::ZERO,
            savings_kind: None,
            cache_affinity: None,
            policy_version: None,
            proxy_ms: None,
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
