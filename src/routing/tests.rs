// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;

#[test]
fn only_retryable_failures_trigger_a_fallback() {
    assert_eq!(fallback_reason(429), Some(FallbackReason::RateLimited));
    assert_eq!(fallback_reason(500), Some(FallbackReason::ServerError));
    assert_eq!(fallback_reason(503), Some(FallbackReason::ServerError));
    // A request problem is surfaced as-is, never retried on another model.
    assert_eq!(fallback_reason(400), None);
    assert_eq!(fallback_reason(401), None);
    assert_eq!(fallback_reason(404), None);
    assert_eq!(fallback_reason(200), None);
}

#[test]
fn a_step_with_no_triggers_keeps_the_old_behaviour() {
    let step = FallbackStep::retryable("deepseek-flash");

    assert_eq!(
        step.triggers,
        FallbackTrigger::RETRYABLE.to_vec(),
        "the guard is opt-in"
    );
}

#[test]
fn legacy_chains_read_as_all_trigger_steps() {
    let stored = serde_json::json!(["gpt-4o-mini", "deepseek-flash"]);

    assert_eq!(
        parse_steps(&stored),
        vec![
            FallbackStep::retryable("gpt-4o-mini"),
            FallbackStep::retryable("deepseek-flash"),
        ]
    );
}

#[test]
fn configured_steps_keep_their_triggers() {
    let stored = serde_json::json!([
        { "model": "deepseek-flash", "triggers": ["rate_limited"] },
        { "model": "gpt-4o-mini", "triggers": ["server_error", "guard"] },
    ]);

    assert_eq!(
        parse_steps(&stored),
        vec![
            FallbackStep::new("deepseek-flash", &[FallbackTrigger::RateLimited]),
            FallbackStep::new(
                "gpt-4o-mini",
                &[FallbackTrigger::ServerError, FallbackTrigger::Guard],
            ),
        ]
    );
}

#[test]
fn a_stored_step_with_empty_triggers_reads_as_retryable() {
    let stored = serde_json::json!([{ "model": "deepseek-flash", "triggers": [] }]);

    assert_eq!(
        parse_steps(&stored),
        vec![FallbackStep::retryable("deepseek-flash")],
        "an old row never widens to the guard"
    );
}

#[test]
fn malformed_chain_values_read_as_no_chain() {
    assert!(parse_steps(&serde_json::json!("nonsense")).is_empty());
    assert!(parse_steps(&serde_json::json!([{ "triggers": ["guard"] }])).is_empty());
    assert!(parse_steps(&serde_json::json!([42])).is_empty());
}

#[test]
fn validation_refuses_empty_models_empty_triggers_and_duplicates() {
    let valid = vec![
        FallbackStep::new("a", &[FallbackTrigger::ServerError]),
        FallbackStep::new("b", &[FallbackTrigger::Transport]),
    ];
    assert_eq!(validate_steps(&valid), Ok(()));

    assert_eq!(
        validate_steps(&[FallbackStep::new("  ", &[FallbackTrigger::Guard])]),
        Err(StepIssue::EmptyModel)
    );
    assert_eq!(
        validate_steps(&[FallbackStep::new("a", &[])]),
        Err(StepIssue::EmptyTriggers)
    );
    assert_eq!(
        validate_steps(&[
            FallbackStep::new("a", &[FallbackTrigger::Guard]),
            FallbackStep::new("a", &[FallbackTrigger::Transport]),
        ]),
        Err(StepIssue::DuplicateModel("a".to_string()))
    );
}

#[test]
fn next_handling_skips_steps_that_do_not_handle_the_failure() {
    let triggers = [
        FallbackTrigger::RETRYABLE.to_vec(),
        vec![FallbackTrigger::ServerError],
        vec![FallbackTrigger::RateLimited],
    ];
    let triggers: Vec<&[FallbackTrigger]> = triggers.iter().map(Vec::as_slice).collect();

    assert_eq!(
        next_handling(&triggers, 0, FallbackTrigger::RateLimited),
        Some(2),
        "a 429 on the primary skips the 5xx-only step"
    );
    assert_eq!(
        next_handling(&triggers, 0, FallbackTrigger::ServerError),
        Some(1)
    );
    assert_eq!(
        next_handling(&triggers, 1, FallbackTrigger::RateLimited),
        Some(2)
    );
    assert_eq!(
        next_handling(&triggers, 2, FallbackTrigger::Transport),
        None,
        "nothing left that handles it"
    );
}

#[test]
fn all_handling_lists_every_matching_step_in_order() {
    let triggers = [
        FallbackTrigger::RETRYABLE.to_vec(),
        vec![FallbackTrigger::Guard],
        vec![FallbackTrigger::ServerError],
        vec![FallbackTrigger::Guard],
    ];
    let triggers: Vec<&[FallbackTrigger]> = triggers.iter().map(Vec::as_slice).collect();

    assert_eq!(
        all_handling(&triggers, 0, FallbackTrigger::Guard),
        vec![1, 3]
    );
    assert!(all_handling(&triggers, 3, FallbackTrigger::Guard).is_empty());
}

#[tokio::test]
async fn chains_round_trip_and_are_tenant_scoped() {
    let tenant = Uuid::new_v4();
    let other = Uuid::new_v4();
    let store = InMemoryRoutingStore::default();
    assert!(
        store.fallbacks(tenant).await.unwrap().is_empty(),
        "no chain by default"
    );

    let steps = vec![
        FallbackStep::new("gpt-4o-mini", &[FallbackTrigger::ServerError]),
        FallbackStep::retryable("deepseek-flash"),
    ];
    store.set_fallbacks(tenant, &steps).await.unwrap();
    assert_eq!(
        store.fallbacks(tenant).await.unwrap(),
        steps,
        "order and triggers are preserved"
    );
    assert!(
        store.fallbacks(other).await.unwrap().is_empty(),
        "other tenant untouched"
    );
}

#[tokio::test]
async fn the_store_refuses_an_invalid_step() {
    let tenant = Uuid::new_v4();
    let store = InMemoryRoutingStore::default();

    let result = store
        .set_fallbacks(tenant, &[FallbackStep::new("deepseek-chat", &[])])
        .await;
    assert!(
        result.is_err(),
        "an empty trigger set never reaches the hot path"
    );
    assert!(store.fallbacks(tenant).await.unwrap().is_empty());
}
