//! The payloads report (ITEM-103): what we do with prompts and completions.
//!
//! A different question from `runtime`'s "is the proxy fast?" — this one is
//! *what happens to the payloads?* Four answers on one page: whether they are
//! stored and for how long, what the pre-flight and batch lints found, what
//! pruning removed, and which PII shapes were detected.
//!
//! Every number here is a count of traces, never a matched value.

use super::*;

/// GET /v1/dashboard/api/payloads?days=30 — capture, lint, pruning and PII for
/// the window, plus the switches currently in force.
pub async fn payloads(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Result<Response, Response> {
    let (tenant_id, _role) = session_membership(&state, &headers).await?;
    let days = window.days();

    // The switches come from the same cached config the hot path reads: no
    // extra query, and the page can never disagree with what the proxy did.
    let budget = state.hotpath.budget(&state, tenant_id).await.ok().flatten();
    let capture = budget.as_ref().is_none_or(|budget| budget.capture_payloads);
    let retention_days = budget.as_ref().map_or(30, |budget| budget.payload_retention_days);
    let lint_enabled = state.hotpath.lint_enabled(&state, tenant_id).await;
    let lint_rules = state.hotpath.lint_rules(&state, tenant_id).await.names();
    let pii = state.hotpath.pii_plan(&state, tenant_id).await;
    let prune = state
        .policy_store
        .active(tenant_id)
        .await
        .ok()
        .flatten()
        .and_then(|policy| policy.prune);

    // A dismissed finding is a false positive the tenant told us about: it keeps
    // its trace, but stops counting as actionable.
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
    let by_kind: Vec<(String, i64)> = stored(sqlx::query_as(
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
    .await)?;

    // The batch linter records `{}` when it finds nothing, so a non-null value
    // means "checked" and `flagged` counts the ones that fired.
    let (checked, flagged): (i64, i64) = stored(sqlx::query_as(
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
    .await)?;
    let batch_by_rule: Vec<(String, i64)> = stored(sqlx::query_as(
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
    .await)?;

    // What pruning removed, and whether the held-out arm answered better.
    // `delta` is pruned minus held-out on the ok rate: negative is a regression.
    let (pruned, tokens_saved, usd_saved, pruned_ok, holdout, holdout_ok): (
        i64,
        i64,
        Decimal,
        i64,
        i64,
        i64,
    ) = stored(sqlx::query_as(
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
    .await)?;
    let rate = |ok: i64, total: i64| (total > 0).then(|| ok as f64 / total as f64);

    // PII counts by class — summed over traces, never over values.
    let (pii_scanned, pii_redacted, pii_wire, pii_total): (i64, i64, i64, i64) = stored(
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
    let pii_by_class: Vec<(String, i64)> = stored(sqlx::query_as(
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
    .await)?;

    let requests: i64 = stored(sqlx::query_scalar(
        "SELECT COUNT(*) FROM traces
         WHERE tenant_id = $1 AND created_at >= NOW() - ($2 * INTERVAL '1 day')",
    )
    .bind(tenant_id)
    .bind(days)
    .fetch_one(&state.db)
    .await)?;

    Ok(json_ok(json!({
        "window": {"days": days},
        "requests": requests,
        "capture": {
            "enabled": capture,
            "retention_days": retention_days,
        },
        "lint": {
            "enabled": lint_enabled,
            "rules": lint_rules,
            "preflight": {
                "scanned": scanned,
                "findings": findings,
                "dismissed": dismissed,
                "wasted_usd": wasted.to_string(),
                "by_kind": by_kind
                    .into_iter()
                    .map(|(kind, count)| (kind, json!(count)))
                    .collect::<serde_json::Map<String, Value>>(),
            },
            "batch": {
                "checked": checked,
                "flagged": flagged,
                "by_rule": batch_by_rule
                    .into_iter()
                    .map(|(rule, count)| (rule, json!(count)))
                    .collect::<serde_json::Map<String, Value>>(),
            },
        },
        "pruning": {
            "policy": prune,
            "rewritten": pruned,
            "holdout_requests": holdout,
            "tokens_saved_est": tokens_saved,
            "usd_saved_est": usd_saved.to_string(),
            "pruned_ok_rate": rate(pruned_ok, pruned),
            "holdout_ok_rate": rate(holdout_ok, holdout),
            "delta": match (rate(pruned_ok, pruned), rate(holdout_ok, holdout)) {
                (Some(pruned), Some(holdout)) => Some(pruned - holdout),
                _ => None,
            },
        },
        "pii": {
            "detection": pii.detect,
            "redact_storage": pii.redact_storage,
            "redact_wire": pii.redact_wire,
            "scanned": pii_scanned,
            "redacted_storage": pii_redacted,
            "redacted_wire": pii_wire,
            "matches": pii_total,
            "by_class": pii_by_class
                .into_iter()
                .map(|(class, count)| (class, json!(count)))
                .collect::<serde_json::Map<String, Value>>(),
        },
    })))
}
