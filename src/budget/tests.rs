    use super::*;

    fn budget(limit: i64, spent: i64, cheap: Option<&str>) -> TenantBudget {
        TenantBudget {
            capture_payloads: true,
            payload_retention_days: 30,
            limit_cents: limit,
            spent_cents: spent,
            cheap_fallback_model: cheap.map(str::to_string),
            guard_enabled: true,
            router_enabled: true,
            router_threshold: 0.35,
            tier_pairs: Vec::new(),
        }
    }

    #[test]
    fn unlimited_and_unconfigured_always_allow() {
        assert_eq!(enforce_budget(&budget(0, 0, None)), BudgetVerdict::Allow);
        assert_eq!(enforce_budget(&budget(0, 99_999, Some("m"))), BudgetVerdict::Allow);
    }

    #[test]
    fn hard_cap_rejects() {
        assert_eq!(enforce_budget(&budget(1000, 1000, None)), BudgetVerdict::Reject);
        assert_eq!(enforce_budget(&budget(1000, 2500, Some("m"))), BudgetVerdict::Reject);
    }

    #[test]
    fn last_fifteen_percent_downgrades_when_model_available() {
        // 84% spent -> Allow (integer math: 840*20 < 1000*17=17000 -> 16800 < 17000)
        assert_eq!(enforce_budget(&budget(1000, 840, Some("gpt-4o-mini"))), BudgetVerdict::Allow);
        // 85% spent -> DowngradeTo
        assert_eq!(
            enforce_budget(&budget(1000, 850, Some("gpt-4o-mini"))),
            BudgetVerdict::DowngradeTo("gpt-4o-mini".to_string())
        );
        // in-zone but no cheap model configured -> Allow (no fallback possible)
        assert_eq!(enforce_budget(&budget(1000, 900, None)), BudgetVerdict::Allow);
    }

    #[tokio::test]
    async fn guard_is_on_by_default_and_opt_out_is_honored() {
        let tenant = uuid::Uuid::new_v4();
        let mut budgets = std::collections::HashMap::new();
        budgets.insert(
            tenant,
            TenantBudget {
                capture_payloads: true,
                payload_retention_days: 30,
                limit_cents: 0,
                spent_cents: 0,
                cheap_fallback_model: None,
                guard_enabled: false,
                router_enabled: true,
                router_threshold: 0.35,
                tier_pairs: Vec::new(),
            },
        );
        let store = InMemoryBudgetStore::new(budgets);
        // Unconfigured tenant (no row) -> guarded (default true).
        assert!(store.guard_enabled_for(uuid::Uuid::new_v4()).await.unwrap());
        // Configured opt-out -> false.
        assert!(!store.guard_enabled_for(tenant).await.unwrap());
    }

    #[tokio::test]
    async fn update_config_upserts_and_preserves_spend() {
        // ITEM-046: the controls write through the same seam the breaker reads.
        let tenant = uuid::Uuid::new_v4();
        let mut budgets = std::collections::HashMap::new();
        budgets.insert(tenant, budget(1000, 250, Some("gpt-4o-mini")));
        let store = InMemoryBudgetStore::new(budgets);
        store
            .update_config(
                tenant,
                &ConfigUpdate {
                    capture_payloads: true,
                    payload_retention_days: 30,
                    budget_limit_cents: 5000,
                    cheap_fallback_model: Some("deepseek-chat".to_string()),
                    guard_enabled: false,
                    router_enabled: false,
                    router_threshold: 0.8,
                },
            )
            .await
            .unwrap();
        let after = store.budget_for(tenant).await.unwrap().unwrap();
        assert_eq!(after.limit_cents, 5000);
        assert_eq!(after.spent_cents, 250, "telemetry owns spend; the update never resets it");
        assert_eq!(after.cheap_fallback_model.as_deref(), Some("deepseek-chat"));
        assert!(!after.guard_enabled);
        assert!(!after.router_enabled);
        assert_eq!(after.router_threshold, 0.8);
        // An unconfigured tenant gets a row on first update (defaults preserved).
        let fresh = uuid::Uuid::new_v4();
        store
            .update_config(
                fresh,
                &ConfigUpdate {
                    capture_payloads: true,
                    payload_retention_days: 30,
                    budget_limit_cents: 0,
                    cheap_fallback_model: None,
                    guard_enabled: true,
                    router_enabled: true,
                    router_threshold: 0.35,
                },
            )
            .await
            .unwrap();
        assert!(store.budget_for(fresh).await.unwrap().is_some());
    }

    #[test]
    fn forward_body_sets_model_and_optionally_downgrades_the_wire_format() {
        let payload = serde_json::json!({
            "model": "deepseek-v4-pro",
            "response_format": {"type": "json_schema", "json_schema": {"name": "a", "schema": {}}},
            "messages": []
        });
        // No downgrade: the contract is forwarded unchanged.
        let kept: Value =
            serde_json::from_slice(&forward_body(&payload, "gpt-4o", false).unwrap()).unwrap();
        assert_eq!(kept["model"], "gpt-4o");
        assert_eq!(kept["response_format"]["type"], "json_schema");
        // Downgrade: model set, wire format json_object.
        let down: Value =
            serde_json::from_slice(&forward_body(&payload, "deepseek-flash", true).unwrap())
                .unwrap();
        assert_eq!(down["model"], "deepseek-flash");
        assert_eq!(
            down["response_format"],
            serde_json::json!({"type": "json_object"})
        );
    }

#[tokio::test]
async fn tier_pairs_survive_a_controls_update_and_work_for_a_fresh_tenant() {
    // ITEM-081: the pairs live on the same row the controls own. A save from
    // the controls panel must not wipe them, and a tenant with no row yet
    // can still declare pairs.
    let store = InMemoryBudgetStore::default();
    let tenant = uuid::Uuid::new_v4();
    let pairs = [crate::router::TierPair {
        frontier: "gpt-4o".into(),
        cheap: "qwen3:8b".into(),
    }];
    store.set_tier_pairs(tenant, &pairs).await.unwrap();
    store
        .update_config(
            tenant,
            &ConfigUpdate {
                capture_payloads: true,
                payload_retention_days: 30,
                budget_limit_cents: 5000,
                cheap_fallback_model: Some("deepseek-chat".to_string()),
                guard_enabled: true,
                router_enabled: true,
                router_threshold: 0.5,
            },
        )
        .await
        .unwrap();
    let budget = store.budget_for(tenant).await.unwrap().unwrap();
    assert_eq!(budget.tier_pairs, pairs);
    assert_eq!(budget.router_threshold, 0.5);
}
