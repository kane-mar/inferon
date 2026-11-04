use super::*;
use crate::budget::{ConfigUpdate, TenantBudget};

fn update() -> ConfigUpdate {
    ConfigUpdate::default()
}

#[test]
fn reports_only_the_fields_that_changed() {
    let before = TenantBudget::unconfigured();
    let after = ConfigUpdate {
        budget_limit_cents: 2_500,
        guard_enabled: false,
        ..update()
    };

    let changes = config_changes(&before, &after);

    assert_eq!(changes.len(), 2, "only the two edited fields");
    assert_eq!(changes[0].setting, "budget_limit_cents");
    assert_eq!(changes[0].before, Some(serde_json::json!(0)));
    assert_eq!(changes[0].after, Some(serde_json::json!(2_500)));
    assert_eq!(changes[1].setting, "guard_enabled");
    assert_eq!(changes[1].before, Some(serde_json::json!(true)));
    assert_eq!(changes[1].after, Some(serde_json::json!(false)));
}

#[test]
fn an_unchanged_save_writes_no_event() {
    assert!(config_changes(&TenantBudget::unconfigured(), &update()).is_empty());
}

#[test]
fn every_control_has_a_setting_name() {
    let before = TenantBudget::unconfigured();
    let after = ConfigUpdate {
        budget_limit_cents: 1,
        cheap_fallback_model: Some("gpt-4o-mini".to_string()),
        guard_enabled: false,
        router_enabled: false,
        router_threshold: 0.9,
        capture_payloads: false,
        payload_retention_days: 7,
        lint_enabled: false,
        lint_rules: vec!["boilerplate".to_string()],
        translate_surfaces: false,
        rpm_limit: 60,
        tpm_limit: 9_000,
        pii_detection: false,
        pii_redact_storage: true,
        pii_redact_wire: true,
    };

    let changes = config_changes(&before, &after);
    let settings: Vec<&str> = changes
        .iter()
        .map(|change| change.setting.as_str())
        .collect();

    assert_eq!(
        settings,
        vec![
            "budget_limit_cents",
            "cheap_fallback_model",
            "guard_enabled",
            "router_enabled",
            "router_threshold",
            "capture_payloads",
            "payload_retention_days",
            "lint_enabled",
            "lint_rules",
            "translate_surfaces",
            "rpm_limit",
            "tpm_limit",
            "pii_detection",
            "pii_redact_storage",
            "pii_redact_wire",
        ]
    );
}

#[tokio::test]
async fn records_events_newest_first_and_per_tenant() {
    let tenant = Uuid::new_v4();
    let other = Uuid::new_v4();
    let actor = Actor {
        id: Uuid::new_v4(),
        login: "kane".to_string(),
    };
    let store = InMemoryConfigAuditStore::default();

    store
        .record(
            tenant,
            &actor,
            &[change("guard_enabled", &true, &false).expect("a real change")],
        )
        .await
        .unwrap();
    store
        .record(
            tenant,
            &actor,
            &[change("router_threshold", &0.35, &0.8).expect("a real change")],
        )
        .await
        .unwrap();

    let recent = store.recent(tenant, 10).await.unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].setting, "router_threshold", "newest first");
    assert_eq!(recent[1].setting, "guard_enabled");
    assert_eq!(recent[0].actor_login, "kane");
    assert!(
        store.recent(other, 10).await.unwrap().is_empty(),
        "tenant-scoped"
    );
}

#[tokio::test]
async fn recent_is_bounded() {
    let tenant = Uuid::new_v4();
    let actor = Actor {
        id: Uuid::new_v4(),
        login: "kane".to_string(),
    };
    let store = InMemoryConfigAuditStore::default();

    for threshold in [0.1, 0.2, 0.3] {
        store
            .record(
                tenant,
                &actor,
                &[change("router_threshold", &0.0, &threshold).expect("a real change")],
            )
            .await
            .unwrap();
    }

    assert_eq!(store.recent(tenant, 2).await.unwrap().len(), 2);
}
