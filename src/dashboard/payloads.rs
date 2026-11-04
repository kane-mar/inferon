//! The payloads report (ITEM-103): what we do with prompts and completions.
//!
//! A different question from `runtime`'s "is the proxy fast?" — this one is
//! *what happens to the payloads?* Four answers on one page: whether they are
//! stored and for how long, what the pre-flight and batch lints found, what
//! pruning removed, and which PII shapes were detected.
//!
//! Every number here is a count of traces, never a matched value.

use super::*;

/// The switches currently in force, read from the same cache the hot path uses.
struct PayloadSwitches {
    capture: bool,
    retention_days: i64,
    lint_enabled: bool,
    lint_rules: Vec<String>,
    pii: crate::proxy::PiiPlan,
    prune: Option<crate::prune::PrunePolicy>,
}

/// The pre-flight lint's counts for the window.
struct LintCounts {
    findings: i64,
    scanned: i64,
    dismissed: i64,
    wasted: Decimal,
    by_kind: Vec<(String, i64)>,
}

/// The batch linter's coverage and findings.
struct BatchCounts {
    checked: i64,
    flagged: i64,
    by_rule: Vec<(String, i64)>,
}

/// The pruning rewrite counts, per arm.
struct PruningCounts {
    rewritten: i64,
    tokens_saved: i64,
    usd_saved: Decimal,
    pruned_ok: i64,
    holdout: i64,
    holdout_ok: i64,
}

/// PII detection/redaction counts. Counts only — never a matched value.
struct PiiCounts {
    scanned: i64,
    redacted_storage: i64,
    redacted_wire: i64,
    total: i64,
    by_class: Vec<(String, i64)>,
}

/// GET /v1/dashboard/api/payloads?days=30 — capture, lint, pruning and PII for
/// the window, plus the switches currently in force.
pub async fn payloads(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Result<Response, Response> {
    let (tenant_id, _role) = session_membership(&state, &headers).await?;
    let days = window.days();

    let switches = switches(&state, tenant_id).await;
    let lint = lint_counts(&state, tenant_id, days).await?;
    let batch = batch_counts(&state, tenant_id, days).await?;
    let pruning = pruning_counts(&state, tenant_id, days).await?;
    let pii = pii_counts(&state, tenant_id, days).await?;
    let requests = request_count(&state, tenant_id, days).await?;

    Ok(json_ok(json!({
        "window": {"days": days},
        "requests": requests,
        "capture": {
            "enabled": switches.capture,
            "retention_days": switches.retention_days,
        },
        "lint": {
            "enabled": switches.lint_enabled,
            "rules": switches.lint_rules,
            "preflight": {
                "scanned": lint.scanned,
                "findings": lint.findings,
                "dismissed": lint.dismissed,
                "wasted_usd": lint.wasted.to_string(),
                "by_kind": count_map(lint.by_kind),
            },
            "batch": {
                "checked": batch.checked,
                "flagged": batch.flagged,
                "by_rule": count_map(batch.by_rule),
            },
        },
        "pruning": {
            "policy": switches.prune,
            "rewritten": pruning.rewritten,
            "holdout_requests": pruning.holdout,
            "tokens_saved_est": pruning.tokens_saved,
            "usd_saved_est": pruning.usd_saved.to_string(),
            "pruned_ok_rate": rate(pruning.pruned_ok, pruning.rewritten),
            "holdout_ok_rate": rate(pruning.holdout_ok, pruning.holdout),
            "delta": pruning_delta(
                pruning.pruned_ok,
                pruning.rewritten,
                pruning.holdout_ok,
                pruning.holdout,
            ),
        },
        "pii": {
            "detection": switches.pii.detect,
            "redact_storage": switches.pii.redact_storage,
            "redact_wire": switches.pii.redact_wire,
            "scanned": pii.scanned,
            "redacted_storage": pii.redacted_storage,
            "redacted_wire": pii.redacted_wire,
            "matches": pii.total,
            "by_class": count_map(pii.by_class),
        },
    })))
}

/// The same cached config the hot path reads: no extra query, and the page can
/// never disagree with what the proxy did.
async fn switches(state: &Arc<AppState>, tenant_id: uuid::Uuid) -> PayloadSwitches {
    let budget = state.hotpath.budget(state, tenant_id).await.ok().flatten();
    let pii = state.hotpath.pii_plan(state, tenant_id).await;
    let prune = state
        .policy_store
        .active(tenant_id)
        .await
        .ok()
        .flatten()
        .and_then(|policy| policy.prune);
    PayloadSwitches {
        capture: budget.as_ref().is_none_or(|budget| budget.capture_payloads),
        retention_days: budget.as_ref().map_or(30, |budget| budget.payload_retention_days),
        lint_enabled: state.hotpath.lint_enabled(state, tenant_id).await,
        lint_rules: state.hotpath.lint_rules(state, tenant_id).await.names(),
        pii,
        prune,
    }
}

/// A dismissed finding is a false positive the tenant told us about: it keeps
/// its trace, but stops counting as actionable.
async fn lint_counts(
    state: &Arc<AppState>,
    tenant_id: uuid::Uuid,
    days: i64,
) -> Result<LintCounts, Response> {
    let found = "jsonb_array_length(request_lint->'kinds') > 0";
    let (findings, scanned, dismissed, wasted): (i64, i64, i64, Decimal) = stored(
        sqlx::query_as(&format!(
            "SELECT
               COUNT(*) FILTER (WHERE {found} AND lint_dismissed_at IS NULL),
               COUNT(*) FILTER (WHERE request_lint IS NOT NULL),
               COUNT(*) FILTER (WHERE {found} AND lint_dismissed_at IS NOT NULL),
               COALESCE(SUM((request_lint->>'wasted_usd_est')::numeric)
                        FILTER (WHERE {found} AND lint_dismissed_at IS NULL), 0)
             FROM traces
             WHERE tenant_id = $1
               AND created_at >= NOW() - ($2 * INTERVAL '1 day')"
        ))
        .bind(tenant_id)
        .bind(days)
        .fetch_one(&state.db)
        .await,
    )?;
    let by_kind = stored(
        sqlx::query_as(
            "SELECT kind, COUNT(*)
             FROM traces, jsonb_array_elements_text(request_lint->'kinds') AS kind
             WHERE tenant_id = $1
               AND created_at >= NOW() - ($2 * INTERVAL '1 day')
               AND request_lint IS NOT NULL
               AND lint_dismissed_at IS NULL
             GROUP BY kind ORDER BY 2 DESC",
        )
        .bind(tenant_id)
        .bind(days)
        .fetch_all(&state.db)
        .await,
    )?;
    Ok(LintCounts {
        findings,
        scanned,
        dismissed,
        wasted,
        by_kind,
    })
}

/// The batch linter records `{}` when it finds nothing, so a non-null value
/// means "checked" and `flagged` counts the ones that fired.
async fn batch_counts(
    state: &Arc<AppState>,
    tenant_id: uuid::Uuid,
    days: i64,
) -> Result<BatchCounts, Response> {
    let (checked, flagged): (i64, i64) = stored(
        sqlx::query_as(
            "SELECT COUNT(*) FILTER (WHERE waste_detected IS NOT NULL),
                COUNT(*) FILTER (WHERE waste_detected IS NOT NULL
                                   AND waste_detected <> '{}'::jsonb)
             FROM traces
             WHERE tenant_id = $1
               AND created_at >= NOW() - ($2 * INTERVAL '1 day')",
        )
        .bind(tenant_id)
        .bind(days)
        .fetch_one(&state.db)
        .await,
    )?;
    let by_rule = stored(
        sqlx::query_as(
            "SELECT flag, COUNT(*)
             FROM traces
             CROSS JOIN LATERAL (VALUES
                ('over_provisioned', waste_detected->>'over_provisioned'),
                ('cache_miss', waste_detected->>'cache_miss_system_prompt'),
                ('model_misuse', waste_detected->>'model_misuse')) AS f(flag, fired)
             WHERE tenant_id = $1
               AND created_at >= NOW() - ($2 * INTERVAL '1 day')
               AND f.fired = 'true'
             GROUP BY flag ORDER BY 2 DESC",
        )
        .bind(tenant_id)
        .bind(days)
        .fetch_all(&state.db)
        .await,
    )?;
    Ok(BatchCounts {
        checked,
        flagged,
        by_rule,
    })
}

/// What pruning removed, and whether the held-out arm answered better.
async fn pruning_counts(
    state: &Arc<AppState>,
    tenant_id: uuid::Uuid,
    days: i64,
) -> Result<PruningCounts, Response> {
    let (rewritten, tokens_saved, usd_saved, pruned_ok, holdout, holdout_ok) = stored(
        sqlx::query_as(
            "SELECT
               COUNT(*) FILTER (WHERE NOT held_out),
               -- SUM(bigint) is numeric in Postgres; cast so it decodes as i64.
               COALESCE(SUM((prune_event->>'tokens_saved_est')::bigint)
                        FILTER (WHERE NOT held_out), 0)::bigint,
               COALESCE(SUM((prune_event->>'usd_saved_est')::numeric)
                        FILTER (WHERE NOT held_out), 0),
               COUNT(*) FILTER (WHERE NOT held_out AND outcome = 'ok'),
               COUNT(*) FILTER (WHERE held_out),
               COUNT(*) FILTER (WHERE held_out AND outcome = 'ok')
             FROM traces,
                  LATERAL (SELECT (prune_event->>'held_out')::boolean AS held_out) AS arm
             WHERE tenant_id = $1
               AND prune_event IS NOT NULL
               AND prune_event->>'held_out' IS NOT NULL
               AND created_at >= NOW() - ($2 * INTERVAL '1 day')",
        )
        .bind(tenant_id)
        .bind(days)
        .fetch_one(&state.db)
        .await,
    )?;
    Ok(PruningCounts {
        rewritten,
        tokens_saved,
        usd_saved,
        pruned_ok,
        holdout,
        holdout_ok,
    })
}

/// PII counts by class — summed over traces, never over values.
async fn pii_counts(
    state: &Arc<AppState>,
    tenant_id: uuid::Uuid,
    days: i64,
) -> Result<PiiCounts, Response> {
    let (scanned, redacted_storage, redacted_wire, total): (i64, i64, i64, i64) = stored(
        sqlx::query_as(
            "SELECT COUNT(*) FILTER (WHERE pii IS NOT NULL),
                    COUNT(*) FILTER (WHERE pii->>'storage' = 'redacted'),
                    COUNT(*) FILTER (WHERE pii->>'wire' = 'redacted'),
                    COALESCE(SUM((pii->>'total')::bigint)
                             FILTER (WHERE pii IS NOT NULL), 0)::bigint
             FROM traces
             WHERE tenant_id = $1
               AND created_at >= NOW() - ($2 * INTERVAL '1 day')",
        )
        .bind(tenant_id)
        .bind(days)
        .fetch_one(&state.db)
        .await,
    )?;
    let by_class = stored(
        sqlx::query_as(
            "SELECT class, SUM(found.count::bigint)::bigint
             FROM traces, jsonb_each_text(pii->'classes') AS found(class, count)
             WHERE tenant_id = $1
               AND created_at >= NOW() - ($2 * INTERVAL '1 day')
               AND pii IS NOT NULL
             GROUP BY class ORDER BY 2 DESC",
        )
        .bind(tenant_id)
        .bind(days)
        .fetch_all(&state.db)
        .await,
    )?;
    Ok(PiiCounts {
        scanned,
        redacted_storage,
        redacted_wire,
        total,
        by_class,
    })
}

async fn request_count(
    state: &Arc<AppState>,
    tenant_id: uuid::Uuid,
    days: i64,
) -> Result<i64, Response> {
    stored(
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM traces
             WHERE tenant_id = $1 AND created_at >= NOW() - ($2 * INTERVAL '1 day')",
        )
        .bind(tenant_id)
        .bind(days)
        .fetch_one(&state.db)
        .await,
    )
}

/// The gap between the pruned and held-out arms: negative is a regression.
fn pruning_delta(
    pruned_ok: i64,
    pruned: i64,
    holdout_ok: i64,
    holdout: i64,
) -> Option<f64> {
    match (rate(pruned_ok, pruned), rate(holdout_ok, holdout)) {
        (Some(pruned), Some(holdout)) => Some(pruned - holdout),
        _ => None,
    }
}

/// Grouped rows as a JSON object, the shape every `by_*` field uses.
fn count_map(rows: Vec<(String, i64)>) -> serde_json::Map<String, Value> {
    rows.into_iter()
        .map(|(name, count)| (name, json!(count)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pruning_delta_is_the_gap_between_the_two_arms() {
        let delta = pruning_delta(9, 10, 8, 10).expect("both arms have samples");
        assert!((delta - 0.1).abs() < 1e-9, "pruned arm is ten points better");

        let regression = pruning_delta(8, 10, 9, 10).expect("both arms have samples");
        assert!((regression + 0.1).abs() < 1e-9, "negative is a regression");
    }

    #[test]
    fn pruning_delta_is_unknown_until_both_arms_have_samples() {
        assert_eq!(pruning_delta(9, 10, 0, 0), None);
        assert_eq!(pruning_delta(0, 0, 8, 10), None);
    }

    #[test]
    fn count_map_turns_grouped_rows_into_a_json_object() {
        let map = count_map(vec![
            ("boilerplate".to_string(), 3),
            ("duplicate_turns".to_string(), 1),
        ]);

        assert_eq!(map.get("boilerplate"), Some(&json!(3)));
        assert_eq!(map.get("duplicate_turns"), Some(&json!(1)));
    }
}
