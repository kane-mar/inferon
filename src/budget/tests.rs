    use super::*;

    fn budget(limit: i64, spent: i64, cheap: Option<&str>) -> TenantBudget {
        TenantBudget {
            capture_payloads: true,
            payload_retention_days: 30,
            lint_enabled: true,
            translate_surfaces: true,
            rpm_limit: 0,
            tpm_limit: 0,
            pii_detection: true,
            pii_redact_storage: false,
            pii_redact_wire: false,
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
                lint_enabled: true,
            translate_surfaces: true,
            rpm_limit: 0,
            tpm_limit: 0,
            pii_detection: true,
            pii_redact_storage: false,
            pii_redact_wire: false,
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
            lint_enabled: true,
            translate_surfaces: true,
            rpm_limit: 0,
            tpm_limit: 0,
            pii_detection: true,
            pii_redact_storage: false,
            pii_redact_wire: false,
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
            lint_enabled: true,
            translate_surfaces: true,
            rpm_limit: 0,
            tpm_limit: 0,
            pii_detection: true,
            pii_redact_storage: false,
            pii_redact_wire: false,
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
                lint_enabled: true,
            translate_surfaces: true,
            rpm_limit: 0,
            tpm_limit: 0,
            pii_detection: true,
            pii_redact_storage: false,
            pii_redact_wire: false,
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

/// The type `table.column` is declared with, as the last migration sets it.
fn declared_type(table: &str, column: &str) -> Option<String> {
    let mut declared = None;
    let mut files: Vec<_> = std::fs::read_dir("db/init")
        .expect("db/init")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
        .collect();
    files.sort();
    for path in files {
        let sql = std::fs::read_to_string(&path).expect("migration is readable");
        let sql = sql
            .lines()
            .filter(|line| !line.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n");
        for statement in sql.split(';') {
            let words: Vec<&str> = statement.split_whitespace().collect();
            let targets_table =
                words.starts_with(&["ALTER", "TABLE", table]) || words.starts_with(&["CREATE", "TABLE", table]);
            let Some(at) = words.iter().position(|word| *word == column) else {
                continue;
            };
            if !targets_table {
                continue;
            }
            if let Some(kind) = words.get(at + 1) {
                // `ADD COLUMN <name> BIGINT` and `ALTER COLUMN <name> TYPE BIGINT`.
                let kind = if *kind == "TYPE" { words.get(at + 2)? } else { kind };
                declared = Some(kind.trim_matches(|c: char| !c.is_alphanumeric()).to_uppercase());
            }
        }
    }
    declared
}

#[test]
fn the_retention_window_is_as_wide_as_the_i64_it_decodes_into() {
    // `TenantBudget::payload_retention_days` is i64. sqlx rejects a narrower
    // column at runtime ("mismatched types ... INT4"), which the in-memory
    // store cannot catch — so the schema itself is the contract.
    assert_eq!(
        declared_type("tenant_configurations", "payload_retention_days").as_deref(),
        Some("BIGINT"),
        "the column must be BIGINT to decode into i64"
    );
}

#[test]
fn the_config_select_covers_every_field_of_the_row() {
    // The row used to be a 16-element tuple decoded by position; a miscount
    // shifted every following field into the wrong type (and shipped twice).
    // Now the query aliases each column and the decoder asks for it by name —
    // this test keeps the two lists identical.
    let expected = [
        "limit_cents",
        "spent_cents",
        "cheap_fallback_model",
        "guard_enabled",
        "router_enabled",
        "router_threshold",
        "tier_pairs",
        "capture_payloads",
        "payload_retention_days",
        "lint_enabled",
        "translate_surfaces",
        "rpm_limit",
        "tpm_limit",
        "pii_detection",
        "pii_redact_storage",
        "pii_redact_wire",
    ];
    let select = crate::budget::CONFIG_SELECT;
    for field in expected {
        assert!(
            select.contains(&format!("AS {field}")),
            "CONFIG_SELECT is missing an alias for {field}:\n{select}"
        );
    }
    assert_eq!(
        select.matches(" AS ").count(),
        expected.len(),
        "every column is aliased, and nothing else is"
    );
}

#[test]
fn no_config_query_has_a_trailing_comma_before_from() {
    // Two config SELECTs grew a column list; a trailing comma before FROM is a
    // syntax error that only Postgres could see (2026-10-31 deploy smoke caught
    // it after the fact — this catches it in the suite instead).
    for (name, source) in [
        ("budget.rs", include_str!("../budget.rs")),
        ("edge.rs", include_str!("../edge.rs")),
    ] {
        let bad: Vec<&str> = source
            .lines()
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|pair| pair[0].trim_end().ends_with(',') && pair[1].trim_start().starts_with("FROM"))
            .map(|pair| pair[0].trim())
            .collect();
        assert!(bad.is_empty(), "{name}: trailing comma before FROM in {bad:?}");
    }
}
