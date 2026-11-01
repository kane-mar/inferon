//! The performance report (ITEM-092): what the proxy itself adds, and whether
//! the sub-millisecond contract holds.
//!
//! The headline is `traces.proxy_us` — request start to just before the first
//! upstream dispatch — because that is the only latency number the proxy owns.
//! End-to-end and TTFT follow, then the hot-path cache's effectiveness (operator
//! only), then the per-model split that explains where the time goes.

use super::*;

use crate::latency::{CONTRACT_WINDOW_HOURS, MIN_SAMPLES, P50_BUDGET_US, P95_GUARD_US};

/// ITEM-099: one model's bucket — `(model, bucket, requests, errors, p95_ms)`.
type HeatCellRow = (String, String, i64, i64, Option<f64>);

/// `(samples, under_1ms, p50, p95, p99)` of the proxy's own overhead.
type OverheadRow = (i64, i64, Option<i64>, Option<i64>, Option<i64>);

/// `(total_p50, total_p95, total_p99, ttft_p50, ttft_p95)` in ms.
type LatencyRow = (
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
);

/// `(model, requests, proxy_p50_us, proxy_p95_us, total_p50_ms, total_p95_ms)`.
type ModelRow = (String, i64, Option<i64>, Option<i64>, Option<f64>, Option<f64>);

/// GET /v1/dashboard/api/performance?days=7 — the proxy's overhead against its
/// contract, the end-to-end latency around it, and the cache behind it.
pub async fn performance(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<super::HeatmapQuery>,
) -> Result<Response, Response> {
    let (tenant_id, _role) = session_membership(&state, &headers).await?;
    let days = query.days();
    let offset = query.offset();
    // ITEM-099: day buckets by default; hour for a short window.
    let step = query.step.clone().unwrap_or_else(|| "day".to_string());

    let (samples, under_1ms, p50_us, p95_us, p99_us): OverheadRow = stored(sqlx::query_as(
        "SELECT COUNT(*),
                COUNT(*) FILTER (WHERE proxy_us < 1000),
                round(percentile_cont(0.5) WITHIN GROUP (ORDER BY proxy_us))::bigint,
                round(percentile_cont(0.95) WITHIN GROUP (ORDER BY proxy_us))::bigint,
                round(percentile_cont(0.99) WITHIN GROUP (ORDER BY proxy_us))::bigint
         FROM traces
         WHERE tenant_id = $1
           AND created_at >= NOW() - (($2 + $3) * INTERVAL '1 day')
           AND created_at < NOW() - ($3 * INTERVAL '1 day')
           AND proxy_us IS NOT NULL",
    )
    .bind(tenant_id)
    .bind(days)
    .bind(offset)
    .fetch_one(&state.db)
    .await)?;

    // The verdict is about the build that is running now: it reads a fixed
    // recent window, so pre-deploy history cannot fail today's claim.
    let (recent_samples, recent_p50, recent_p95): (i64, Option<i64>, Option<i64>) = stored(
        sqlx::query_as(
            "SELECT COUNT(*),
                    round(percentile_cont(0.5) WITHIN GROUP (ORDER BY proxy_us))::bigint,
                    round(percentile_cont(0.95) WITHIN GROUP (ORDER BY proxy_us))::bigint
             FROM traces
             WHERE tenant_id = $1
               AND created_at >= NOW() - ($2 * INTERVAL '1 hour')
               AND proxy_us IS NOT NULL",
        )
        .bind(tenant_id)
        .bind(CONTRACT_WINDOW_HOURS)
        .fetch_one(&state.db)
        .await,
    )?;
    let contract = crate::latency::verdict(recent_samples, recent_p50, recent_p95);

    let (total_p50, total_p95, total_p99, ttft_p50, ttft_p95): LatencyRow = stored(sqlx::query_as(
        "SELECT round(percentile_cont(0.5) WITHIN GROUP (ORDER BY total_duration_ms))::float8,
                round(percentile_cont(0.95) WITHIN GROUP (ORDER BY total_duration_ms))::float8,
                round(percentile_cont(0.99) WITHIN GROUP (ORDER BY total_duration_ms))::float8,
                round(percentile_cont(0.5) WITHIN GROUP (ORDER BY time_to_first_token_ms))::float8,
                round(percentile_cont(0.95) WITHIN GROUP (ORDER BY time_to_first_token_ms))::float8
         FROM traces
         WHERE tenant_id = $1
           AND created_at >= NOW() - (($2 + $3) * INTERVAL '1 day')
           AND created_at < NOW() - ($3 * INTERVAL '1 day')",
    )
    .bind(tenant_id)
    .bind(days)
    .bind(offset)
    .fetch_one(&state.db)
    .await)?;

    let models: Vec<ModelRow> = stored(sqlx::query_as(
        "SELECT request_model, COUNT(*),
                round(percentile_cont(0.5) WITHIN GROUP (ORDER BY proxy_us))::bigint,
                round(percentile_cont(0.95) WITHIN GROUP (ORDER BY proxy_us))::bigint,
                round(percentile_cont(0.5) WITHIN GROUP (ORDER BY total_duration_ms))::float8,
                round(percentile_cont(0.95) WITHIN GROUP (ORDER BY total_duration_ms))::float8
         FROM traces
         WHERE tenant_id = $1
           AND created_at >= NOW() - (($2 + $3) * INTERVAL '1 day')
           AND created_at < NOW() - ($3 * INTERVAL '1 day')
           AND proxy_us IS NOT NULL
         GROUP BY request_model ORDER BY 2 DESC LIMIT 10",
    )
    .bind(tenant_id)
    .bind(days)
    .bind(offset)
    .fetch_all(&state.db)
    .await)?;

    // ITEM-090/092: linting has to be *observable* — coverage and outcome, not
    // just findings. Every scanned request records `request_lint` (empty kinds
    // when clean) and every checked trace records `waste_detected` (`{}` when
    // clean), so "ran and found nothing" is distinguishable from "never ran".
    let found = "jsonb_array_length(request_lint->'kinds') > 0";
    let (linted, scanned, dismissed, wasted): (i64, i64, i64, Decimal) = stored(sqlx::query_as(&format!(
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
    .await)?;
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

    // ITEM-091: what pruning removed, and whether the held-out arm answered
    // better. `delta` is pruned minus held-out on the ok rate — negative is a
    // regression and the kill switch has already fired by the time it is large.
    let pruning: (i64, i64, Decimal, i64, i64, i64, i64) = stored(sqlx::query_as(
        "SELECT
           COUNT(*) FILTER (WHERE NOT held_out),
           -- SUM(bigint) is numeric in Postgres; cast so it decodes as i64.
           COALESCE(SUM((prune_event->>'tokens_saved_est')::bigint)
                    FILTER (WHERE NOT held_out), 0)::bigint,
           COALESCE(SUM((prune_event->>'usd_saved_est')::numeric)
                    FILTER (WHERE NOT held_out), 0),
           COUNT(*) FILTER (WHERE NOT held_out AND outcome = 'ok'),
           COUNT(*) FILTER (WHERE held_out),
           COUNT(*) FILTER (WHERE held_out AND outcome = 'ok'),
           COUNT(*)
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
    let (pruned, tokens_saved, usd_saved, pruned_ok, holdout, holdout_ok, pruned_and_holdout) = pruning;
    let rate = |ok: i64, total: i64| (total > 0).then(|| ok as f64 / total as f64);

    // ITEM-099: one bounded aggregate for the matrix; the pivot is pure.
    let heat: Vec<HeatCellRow> = stored(sqlx::query_as(&format!(
        "SELECT request_model,
                to_char(date_trunc('{step}', created_at), 'YYYY-MM-DD HH24'),
                COUNT(*),
                COUNT(*) FILTER (WHERE status_code >= 400),
                percentile_cont(0.95) WITHIN GROUP (ORDER BY total_duration_ms)::float8
         FROM traces
         WHERE tenant_id = $1
           AND created_at >= NOW() - ($2 * INTERVAL '1 day')
         GROUP BY 1, 2 ORDER BY 1, 2",
        step = crate::heatmap::bucket_expr(&step)
    ))
    .bind(tenant_id)
    .bind(days)
    .fetch_all(&state.db)
    .await)?;
    let buckets: Vec<String> = heat
        .iter()
        .map(|(_, bucket, _, _, _)| bucket.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let cells: Vec<(String, String, crate::heatmap::Cell)> = heat
        .into_iter()
        .map(|(model, bucket, requests, errors, p95_ms)| {
            (
                model,
                bucket,
                crate::heatmap::Cell {
                    requests,
                    errors,
                    p95_ms,
                },
            )
        })
        .collect();
    let matrix: Vec<Value> = crate::heatmap::pivot(&buckets, &cells)
        .into_iter()
        .map(|(model, cells)| {
            json!({
                "model": model,
                "cells": cells.into_iter().map(|cell| match cell {
                    Some(cell) => json!({
                        "requests": cell.requests,
                        "errors": cell.errors,
                        "error_rate": cell.error_rate(),
                        "p95_ms": cell.p95_ms,
                    }),
                    None => Value::Null,
                }).collect::<Vec<_>>(),
            })
        })
        .collect();

    // ITEM-096: PII counts by class, and how much of the traffic was redacted.
    // `by_class` is summed over traces, never over values.
    let (pii_scanned, pii_redacted, pii_wire, pii_total): (i64, i64, i64, i64) =
        stored(sqlx::query_as(
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
        .await)?;
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

    // The batch linter (rules A/B/C) records `{}` when it finds nothing, so a
    // non-NULL value is "checked". Coverage and hit rate in one query.
    let (batch_checked, batch_flagged): (i64, i64) = stored(sqlx::query_as(
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
    let requests_in_window: i64 = stored(sqlx::query_scalar(
        "SELECT COUNT(*) FROM traces
         WHERE tenant_id = $1 AND created_at >= NOW() - ($2 * INTERVAL '1 day')",
    )
    .bind(tenant_id)
    .bind(days)
    .fetch_one(&state.db)
    .await)?;
    let preflight_enabled = state.hotpath.lint_enabled(&state, tenant_id).await;

    // Process-wide counters are operator information, not a tenant's.
    let cache = match super::operator_or(&state, &headers).await {
        Ok(()) => Some(cache_json(state.hotpath.stats())),
        Err(_) => None,
    };

    let under_1ms_rate = (samples > 0).then(|| under_1ms as f64 / samples as f64);
    // `overhead` describes the window; the verdict (`contract`) is the recent one.
    let proxy_share = match (p50_us, total_p50) {
        (Some(proxy), Some(total)) if total > 0.0 => Some(proxy as f64 / (total * 1000.0)),
        _ => None,
    };

    Ok(json_ok(json!({
        "window": {"days": days, "offset_days": offset},
        "contract": {
            "p50_us": P50_BUDGET_US,
            "p95_us": P95_GUARD_US,
            "min_samples": MIN_SAMPLES,
            "window_hours": CONTRACT_WINDOW_HOURS,
            "samples": contract.samples,
            "observed_p50_us": contract.p50_us,
            "observed_p95_us": contract.p95_us,
            "p50_met": contract.p50_met,
            "p95_met": contract.p95_met,
        },
        "overhead": {
            "samples": samples,
            "under_1ms": under_1ms,
            "under_1ms_rate": under_1ms_rate,
            "p50_us": p50_us,
            "p95_us": p95_us,
            "p99_us": p99_us,
        },
        "latency": {
            "total_p50_ms": total_p50,
            "total_p95_ms": total_p95,
            "total_p99_ms": total_p99,
            "ttft_p50_ms": ttft_p50,
            "ttft_p95_ms": ttft_p95,
            "proxy_share": proxy_share,
        },
        "lint": {
            "enabled": preflight_enabled,
            "requests": requests_in_window,
            "preflight": {
                "scanned": scanned,
                "findings": linted,
                "dismissed": dismissed,
                "wasted_usd": wasted.to_string(),
            },
            "batch": {
                "checked": batch_checked,
                "flagged": batch_flagged,
                "by_rule": batch_by_rule
                    .into_iter()
                    .map(|(rule, count)| (rule, json!(count)))
                    .collect::<serde_json::Map<String, Value>>(),
            },
            "dismissed": dismissed,
            "wasted_usd": wasted.to_string(),
            "by_kind": by_kind
                .into_iter()
                .map(|(kind, count)| (kind, json!(count)))
                .collect::<serde_json::Map<String, Value>>(),
        },
        "pruning": {
            "enabled": pruned_and_holdout > 0,
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
        "heatmap": {
            "step": crate::heatmap::bucket_expr(&step),
            "buckets": buckets,
            "models": matrix,
        },
        "pii": {
            "scanned": pii_scanned,
            "redacted_storage": pii_redacted,
            "redacted_wire": pii_wire,
            "matches": pii_total,
            "by_class": pii_by_class
                .into_iter()
                .map(|(class, count)| (class, json!(count)))
                .collect::<serde_json::Map<String, Value>>(),
        },
        "cache": cache,
        "by_model": models.into_iter().map(|(model, requests, proxy_p50, proxy_p95, total_p50, total_p95)| json!({
            "model": model,
            "requests": requests,
            "proxy_p50_us": proxy_p50,
            "proxy_p95_us": proxy_p95,
            "total_p50_ms": total_p50,
            "total_p95_ms": total_p95,
        })).collect::<Vec<_>>(),
    })))
}

pub(crate) fn cache_json(stats: crate::hotpath::CacheStats) -> Value {
    let count = |count: crate::hotpath::CacheCount| {
        json!({
            "hits": count.hits,
            "misses": count.misses,
            "hit_rate": count.hit_rate(),
        })
    };
    json!({
        "auth": count(stats.auth),
        "config": count(stats.config),
        "route": count(stats.route),
        "warm": count(stats.warm),
        "spend": count(stats.spend),
        "invalidations": stats.invalidations,
        "tenants": stats.tenants,
        "oldest_age_ms": stats.oldest_age_ms,
    })
}

/// ITEM-099: the performance report also takes a bucket step for the matrix.
/// Flat on purpose — `serde_urlencoded` cannot flatten into a nested struct.
#[derive(Debug, serde::Deserialize)]
pub struct HeatmapQuery {
    days: Option<i64>,
    offset: Option<i64>,
    step: Option<String>,
}

impl HeatmapQuery {
    fn days(&self) -> i64 {
        self.days.unwrap_or(7).clamp(1, 365)
    }
    fn offset(&self) -> i64 {
        self.offset.unwrap_or(0).clamp(0, 3650)
    }
}

