//! The performance report (ITEM-092): what the proxy itself adds, and whether
//! the sub-millisecond contract holds.
//!
//! The headline is `traces.proxy_us` — request start to just before the first
//! upstream dispatch — because that is the only latency number the proxy owns.
//! End-to-end and TTFT follow, then the hot-path cache's effectiveness (operator
//! only), then the per-model split that explains where the time goes.

use super::*;

/// The contract, in microseconds: the claim is p50, the tail is a guard against
/// a broken cache (which measured 6–28 ms) rather than a scheduling bound.
const P50_BUDGET_US: i64 = 1_000;
const P95_GUARD_US: i64 = 5_000;
/// A percentile over a handful of samples is not a verdict. Below this the
/// report says "not enough samples" rather than met/not-met.
const MIN_SAMPLES: i64 = 20;

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
    Query(window): Query<Window>,
) -> Result<Response, Response> {
    let (tenant_id, _role) = session_membership(&state, &headers).await?;
    let days = window.days();
    let offset = window.offset();

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

    // A dismissed finding is a false positive the tenant told us about: it keeps
    // its trace, but stops counting as actionable (ITEM-090 AC5).
    let (linted, dismissed, wasted): (i64, i64, Decimal) = stored(sqlx::query_as(
        "SELECT
           COUNT(*) FILTER (WHERE request_lint IS NOT NULL AND lint_dismissed_at IS NULL),
           COUNT(*) FILTER (WHERE lint_dismissed_at IS NOT NULL),
           COALESCE(SUM((request_lint->>'wasted_usd_est')::numeric)
                    FILTER (WHERE request_lint IS NOT NULL AND lint_dismissed_at IS NULL), 0)
         FROM traces
         WHERE tenant_id = $1
           AND created_at >= NOW() - ($2 * INTERVAL '1 day')",
    )
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
           COALESCE(SUM((prune_event->>'tokens_saved_est')::bigint)
                    FILTER (WHERE NOT held_out), 0),
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

    // Process-wide counters are operator information, not a tenant's.
    let cache = match super::operator_or(&state, &headers).await {
        Ok(()) => Some(cache_json(state.hotpath.stats())),
        Err(_) => None,
    };

    let under_1ms_rate = (samples > 0).then(|| under_1ms as f64 / samples as f64);
    let sufficient = samples >= MIN_SAMPLES;
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
            "p50_met": sufficient.then(|| p50_us.is_some_and(|p50| p50 < P50_BUDGET_US)),
            "p95_met": sufficient.then(|| p95_us.is_some_and(|p95| p95 < P95_GUARD_US)),
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
            "requests": linted,
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

fn cache_json(stats: crate::hotpath::CacheStats) -> Value {
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
    })
}
