// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! The runtime report (ITEM-103): what the proxy itself adds, and whether the
//! sub-millisecond contract holds.
//!
//! One question — *is the proxy fast?* — so one page: the contract, the proxy's
//! own overhead, the end-to-end latency around it, the model × time matrix that
//! shows degradation, and the per-model split that explains where the time goes.
//! Payload governance lives in `payloads`; it is a different question.

use super::*;

use crate::latency::{
    band_bounds, BANDS, CONTRACT_WINDOW_HOURS, MIN_SAMPLES, P50_BUDGET_US, P95_GUARD_US,
    SMALL_PROMPT_TOKENS,
};

/// ITEM-099: one model's bucket — `(model, bucket, requests, errors, p95_ms)`.
type HeatCellRow = (String, String, i64, i64, Option<f64>);

/// `(samples, under_1ms, p50, p95, p99, prompt_tokens_p50, receipt_p50)` of the
/// proxy's own overhead, plus how long clients took to send their bodies
/// (ITEM-135 — a slow upload is not gateway work).
/// `(samples, under_1ms, p50_us, p95_us, p99_us, prompt_tokens_p50)`.
///
/// ITEM-127: gateway time is proportional to the payload — parsing the body is
/// unavoidable work — so the number is meaningless without the prompt size it
/// was measured at. It travels with the percentile, never apart from it.
type OverheadRow = (
    i64,
    i64,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
);

/// `(total_p50, total_p95, total_p99, ttft_p50, ttft_p95)` in ms.
type LatencyRow = (
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
);

/// `(model, requests, proxy_p50_us, proxy_p95_us, total_p50_ms, total_p95_ms)`.
/// `(model, requests, proxy_p50_us, proxy_p95_us, total_p50_ms, total_p95_ms,
/// prompt_tokens_p50, body_p50_ms)` — the payload context and the client's upload
/// ride with the gateway time (ITEM-136).
type ModelRow = (
    String,
    i64,
    Option<i64>,
    Option<i64>,
    Option<f64>,
    Option<f64>,
    Option<i64>,
    Option<i64>,
);

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

    let (samples, under_1ms, p50_us, p95_us, p99_us, prompt_tokens_p50, body_p50, body_p95) =
        overhead(&state, tenant_id, days, offset).await?;
    let verdict = contract_verdict(&state, tenant_id).await?;
    let (total_p50, total_p95, total_p99, ttft_p50, ttft_p95) =
        latency(&state, tenant_id, days, offset).await?;
    let models = model_rows(&state, tenant_id, days, offset).await?;
    let bands = bands(&state, tenant_id, days, offset).await?;
    let (since, pre_era) = measured_since(&state, tenant_id).await?;
    let (bucket_expr, buckets, matrix) = heatmap(&state, tenant_id, days, &step).await?;

    Ok(json_ok(json!({
        "window": {"days": days, "offset_days": offset},
        "contract": {
            "p50_us": P50_BUDGET_US,
            "p95_us": P95_GUARD_US,
            "min_samples": MIN_SAMPLES,
            "window_hours": CONTRACT_WINDOW_HOURS,
            "samples": verdict.samples,
            "observed_p50_us": verdict.p50_us,
            "observed_p95_us": verdict.p95_us,
            "prompt_tokens_p50": verdict.prompt_tokens_p50,
            "p50_met": verdict.p50_met,
            "p95_met": verdict.p95_met,
            // ITEM-136: what the verdict was actually computed over. Without this
            // an operator cannot tell "no small-prompt traffic" from "failing".
            "basis": {
                "max_tokens": SMALL_PROMPT_TOKENS,
                "measured_since": since,
                // ITEM-136: traffic the report excludes because it predates the
                // clock change, so an empty page can explain itself.
                "excluded_requests": pre_era,
            },
        },
        "overhead": {
            "samples": samples,
            "under_1ms": under_1ms,
            "under_1ms_rate": rate(under_1ms, samples),
            "p50_us": p50_us,
            "p95_us": p95_us,
            "p99_us": p99_us,
            "prompt_tokens_p50": prompt_tokens_p50,
            // ITEM-135: the client's side of the same requests, so an operator can
            // see a slow sender instead of reading it as proxy latency.
            "body_p50_ms": body_p50,
            "body_p95_ms": body_p95,
        },
        "latency": {
            "total_p50_ms": total_p50,
            "total_p95_ms": total_p95,
            "total_p99_ms": total_p99,
            "ttft_p50_ms": ttft_p50,
            "ttft_p95_ms": ttft_p95,
            "proxy_share": proxy_share(p50_us, total_p50),
        },
        "heatmap": {
            "step": bucket_expr,
            "buckets": buckets,
            "models": matrix,
        },
        "by_model": models,
        // ITEM-136: the gateway's cost, band by band, with the client's upload.
        "bands": bands,
    })))
}

async fn overhead(
    state: &Arc<AppState>,
    tenant_id: uuid::Uuid,
    days: i64,
    offset: i64,
) -> Result<OverheadRow, Response> {
    stored(
        sqlx::query_as(
            "SELECT COUNT(*),
                COUNT(*) FILTER (WHERE proxy_us < 1000),
                round(percentile_cont(0.5) WITHIN GROUP (ORDER BY proxy_us))::bigint,
                round(percentile_cont(0.95) WITHIN GROUP (ORDER BY proxy_us))::bigint,
                round(percentile_cont(0.99) WITHIN GROUP (ORDER BY proxy_us))::bigint,
                round(percentile_cont(0.5) WITHIN GROUP (ORDER BY prompt_tokens))::bigint,
                round(percentile_cont(0.5) WITHIN GROUP (ORDER BY body_ms))::bigint,
                round(percentile_cont(0.95) WITHIN GROUP (ORDER BY body_ms))::bigint
         FROM traces
         WHERE tenant_id = $1
           AND created_at >= NOW() - (($2 + $3) * INTERVAL '1 day')
           AND created_at < NOW() - ($3 * INTERVAL '1 day')
           AND proxy_us IS NOT NULL
           -- ITEM-135/136: rows written before `body_ms` existed measured the
           -- client's upload as gateway time. Blending the two eras turns a
           -- measurement change into an apparent regression.
           AND body_ms IS NOT NULL",
        )
        .bind(tenant_id)
        .bind(days)
        .bind(offset)
        .fetch_one(&state.db)
        .await,
    )
}

/// The verdict is about the build that is running now: it reads a fixed recent
/// window, so pre-deploy history cannot fail today's claim.
async fn contract_verdict(
    state: &Arc<AppState>,
    tenant_id: uuid::Uuid,
) -> Result<crate::latency::Verdict, Response> {
    // ITEM-136: the claim is p50 < 1 ms "for a warm request carrying a small
    // prompt". Judging a 285k-token request against it was the report's worst
    // lie — the number was real, the comparison was not. So the verdict reads
    // only traffic inside the claim's own payload basis, and only traffic
    // measured by the current clock (ITEM-135: rows before `body_ms` include the
    // client's upload).
    let (samples, p50, p95, prompt_tokens_p50): (i64, Option<i64>, Option<i64>, Option<i64>) =
        stored(
        sqlx::query_as(
            "SELECT COUNT(*),
                    round(percentile_cont(0.5) WITHIN GROUP (ORDER BY proxy_us))::bigint,
                    round(percentile_cont(0.95) WITHIN GROUP (ORDER BY proxy_us))::bigint,
                    round(percentile_cont(0.5) WITHIN GROUP (ORDER BY prompt_tokens))::bigint
             FROM traces
             WHERE tenant_id = $1
               AND created_at >= NOW() - ($2 * INTERVAL '1 hour')
               AND proxy_us IS NOT NULL
               AND body_ms IS NOT NULL
               AND prompt_tokens <= $3",
        )
        .bind(tenant_id)
        .bind(CONTRACT_WINDOW_HOURS)
        .bind(SMALL_PROMPT_TOKENS)
        .fetch_one(&state.db)
        .await,
    )?;
    Ok(crate::latency::verdict(samples, p50, p95).with_payload(prompt_tokens_p50))
}

/// ITEM-136: `(requests, under_1ms, p50_us, p95_us, prompt_tokens_p50,
/// body_p50_ms)` for one payload band; the band's name comes from the constant,
/// not the database.
type BandRow = (i64, i64, Option<i64>, Option<i64>, Option<i64>, Option<i64>);

/// Gateway time per payload band, with the client's upload beside it.
///
/// One blended percentile describes neither end of the traffic: with a 1.2 MB
/// prompt in the mix the p50 is the parse cost of a big body, and an operator
/// cannot tell a regression from a change in what people send. Each band is
/// measured on its own, and the upload column says how much of the elapsed time
/// was never ours.
async fn bands(
    state: &Arc<AppState>,
    tenant_id: uuid::Uuid,
    days: i64,
    offset: i64,
) -> Result<Vec<Value>, Response> {
    let mut out = Vec::new();
    for (index, band) in BANDS.iter().enumerate() {
        let (lower, upper) = band_bounds(index);
        let row: BandRow = stored(
            sqlx::query_as(
                "SELECT COUNT(*),
                    COUNT(*) FILTER (WHERE proxy_us < 1000),
                    round(percentile_cont(0.5) WITHIN GROUP (ORDER BY proxy_us))::bigint,
                    round(percentile_cont(0.95) WITHIN GROUP (ORDER BY proxy_us))::bigint,
                    round(percentile_cont(0.5) WITHIN GROUP (ORDER BY prompt_tokens))::bigint,
                    round(percentile_cont(0.5) WITHIN GROUP (ORDER BY body_ms))::bigint
                 FROM traces
                 WHERE tenant_id = $1
                   AND created_at >= NOW() - (($2 + $3) * INTERVAL '1 day')
                   AND created_at < NOW() - ($3 * INTERVAL '1 day')
                   AND proxy_us IS NOT NULL
                   AND body_ms IS NOT NULL
                   AND prompt_tokens >= $4
                   AND ($5::bigint IS NULL OR prompt_tokens <= $5)",
            )
            .bind(tenant_id)
            .bind(days)
            .bind(offset)
            .bind(lower)
            .bind(upper)
            .fetch_one(&state.db)
            .await,
        )?;
        let (requests, under_1ms, p50_us, p95_us, tokens, upload) = row;
        // An empty band is not a zero: say nothing rather than print a healthy
        // 0 ms for traffic nobody sent.
        if requests == 0 {
            continue;
        }
        out.push(json!({
            "band": band.label,
            "max_tokens": band.max_tokens,
            "requests": requests,
            "under_1ms_rate": rate(under_1ms, requests),
            "p50_us": p50_us,
            "p95_us": p95_us,
            "prompt_tokens_p50": tokens,
            "body_p50_ms": upload,
        }));
    }
    Ok(out)
}

/// When this tenant's traffic started being measured by the current clock
/// (ITEM-135), and how much traffic the report now excludes as a result.
///
/// Returning the excluded count is the difference between a report that looks
/// empty and a report that explains itself: without it a reader sees zero
/// samples and assumes the page is broken, when in fact their traffic predates
/// the measurement change.
async fn measured_since(
    state: &Arc<AppState>,
    tenant_id: uuid::Uuid,
) -> Result<(Option<String>, i64), Response> {
    // Rendered as ISO-8601 in SQL: the pool's type set has no timestamp decoder,
    // and a string is what the page prints anyway.
    let row: (Option<String>, i64) = stored(
        sqlx::query_as(
            "SELECT to_char(MIN(created_at) FILTER (WHERE body_ms IS NOT NULL)
                            AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
                    COUNT(*) FILTER (WHERE body_ms IS NULL)
             FROM traces
             WHERE tenant_id = $1
               AND proxy_us IS NOT NULL
               AND created_at >= NOW() - INTERVAL '30 days'",
        )
        .bind(tenant_id)
        .fetch_one(&state.db)
        .await,
    )?;
    Ok(row)
}

async fn latency(
    state: &Arc<AppState>,
    tenant_id: uuid::Uuid,
    days: i64,
    offset: i64,
) -> Result<LatencyRow, Response> {
    stored(
        sqlx::query_as(
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
        .await,
    )
}

async fn model_rows(
    state: &Arc<AppState>,
    tenant_id: uuid::Uuid,
    days: i64,
    offset: i64,
) -> Result<Vec<Value>, Response> {
    let rows: Vec<ModelRow> = stored(
        sqlx::query_as(
            "SELECT request_model, COUNT(*),
                round(percentile_cont(0.5) WITHIN GROUP (ORDER BY proxy_us))::bigint,
                round(percentile_cont(0.95) WITHIN GROUP (ORDER BY proxy_us))::bigint,
                round(percentile_cont(0.5) WITHIN GROUP (ORDER BY total_duration_ms))::float8,
                round(percentile_cont(0.95) WITHIN GROUP (ORDER BY total_duration_ms))::float8,
                round(percentile_cont(0.5) WITHIN GROUP (ORDER BY prompt_tokens))::bigint,
                round(percentile_cont(0.5) WITHIN GROUP (ORDER BY body_ms))::bigint
         FROM traces
         WHERE tenant_id = $1
           AND created_at >= NOW() - (($2 + $3) * INTERVAL '1 day')
           AND created_at < NOW() - ($3 * INTERVAL '1 day')
           AND proxy_us IS NOT NULL
           -- ITEM-136: same era rule as the headline. A per-model table that
           -- averages rows from before the clock change reintroduces the exact
           -- misreport the bands exist to remove.
           AND body_ms IS NOT NULL
         GROUP BY request_model ORDER BY 2 DESC LIMIT 10",
        )
        .bind(tenant_id)
        .bind(days)
        .bind(offset)
        .fetch_all(&state.db)
        .await,
    )?;
    Ok(rows
        .into_iter()
        .map(
            |(model, requests, proxy_p50, proxy_p95, total_p50, total_p95, tokens_p50, upload)| {
                json!({
                    "model": model,
                    "requests": requests,
                    "proxy_p50_us": proxy_p50,
                    "proxy_p95_us": proxy_p95,
                    "total_p50_ms": total_p50,
                    "total_p95_ms": total_p95,
                    "prompt_tokens_p50": tokens_p50,
                    "body_p50_ms": upload,
                })
            },
        )
        .collect())
}

/// ITEM-099: one bounded aggregate for the matrix; the pivot is pure.
async fn heatmap(
    state: &Arc<AppState>,
    tenant_id: uuid::Uuid,
    days: i64,
    step: &str,
) -> Result<(String, Vec<String>, Vec<Value>), Response> {
    let heat: Vec<HeatCellRow> = stored(
        sqlx::query_as(&format!(
            "SELECT request_model,
                to_char(date_trunc('{step}', created_at), 'YYYY-MM-DD HH24'),
                COUNT(*),
                COUNT(*) FILTER (WHERE status_code >= 400),
                percentile_cont(0.95) WITHIN GROUP (ORDER BY total_duration_ms)::float8
         FROM traces
         WHERE tenant_id = $1
           AND created_at >= NOW() - ($2 * INTERVAL '1 day')
         GROUP BY 1, 2 ORDER BY 1, 2",
            step = crate::heatmap::bucket_expr(step)
        ))
        .bind(tenant_id)
        .bind(days)
        .fetch_all(&state.db)
        .await,
    )?;
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
    Ok((crate::heatmap::bucket_expr(step).to_string(), buckets, matrix))
}

/// The proxy's own p50 as a share of the end-to-end p50 (both in µs once the
/// millisecond total is converted).
fn proxy_share(p50_us: Option<i64>, total_p50_ms: Option<f64>) -> Option<f64> {
    match (p50_us, total_p50_ms) {
        (Some(proxy), Some(total)) if total > 0.0 => Some(proxy as f64 / (total * 1000.0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_share_is_the_proxys_share_of_end_to_end() {
        let share = proxy_share(Some(500), Some(100.0)).expect("both percentiles exist");
        assert!((share - 0.005).abs() < 1e-9, "500 µs of 100 ms");
    }

    #[test]
    fn proxy_share_is_unknown_without_a_positive_total() {
        assert_eq!(proxy_share(Some(500), Some(0.0)), None);
        assert_eq!(proxy_share(None, Some(100.0)), None);
        assert_eq!(proxy_share(Some(500), None), None);
    }
}
