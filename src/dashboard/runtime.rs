//! The runtime report (ITEM-103): what the proxy itself adds, and whether the
//! sub-millisecond contract holds.
//!
//! One question — *is the proxy fast?* — so one page: the contract, the proxy's
//! own overhead, the end-to-end latency around it, the model × time matrix that
//! shows degradation, and the per-model split that explains where the time goes.
//! Payload governance lives in `payloads`; it is a different question.

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

/// The report's window: `days` back, ending `offset` days before now.
#[derive(serde::Deserialize)]
pub struct RuntimeQuery {
    days: Option<i64>,
    offset: Option<i64>,
    /// ITEM-099: `day` (default) or `hour` for the matrix.
    step: Option<String>,
}

impl RuntimeQuery {
    fn days(&self) -> i64 {
        self.days.unwrap_or(7).clamp(1, 365)
    }
    fn offset(&self) -> i64 {
        self.offset.unwrap_or(0).clamp(0, 3650)
    }
}

/// GET /v1/dashboard/api/runtime — the proxy's overhead against its contract,
/// the end-to-end latency around it, and the per-model split.
pub async fn runtime(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<RuntimeQuery>,
) -> Result<Response, Response> {
    let (tenant_id, _role) = session_membership(&state, &headers).await?;
    let days = query.days();
    let offset = query.offset();
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

    let under_1ms_rate = (samples > 0).then(|| under_1ms as f64 / samples as f64);
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
        "heatmap": {
            "step": crate::heatmap::bucket_expr(&step),
            "buckets": buckets,
            "models": matrix,
        },
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
