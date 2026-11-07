// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! The tenant aggregate queries and their read endpoints.

use super::*;

/// `(requests, cost, errors, prompt_tokens, completion_tokens, avg_ttft,
/// avg_duration, first_at, last_at, cached_tokens, cache_savings)` — the
/// summary aggregate row. Latency percentiles come from a second query
/// (`LatencyRow`) because sqlx's tuple `FromRow` tops out at 16 columns.
type SummaryRow = (
    i64,
    Decimal,
    i64,
    i64,
    i64,
    Option<f64>,
    Option<f64>,
    Option<String>,
    Option<String>,
    i64,
    Decimal,
    i64,
    Decimal,
    Decimal,
    Decimal,
    i64,
);

/// `(ttft_p50, ttft_p95, ttft_p99, duration_p50, duration_p95, duration_p99)`.
type LatencyRow = (
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
);

/// ITEM-058 `(model, requests, prompt_tokens, completion_tokens, cost,
/// cached_tokens, cache_savings, savings)` — one spend-by-model row.
type ModelAggregateRow = (String, i64, i64, i64, Decimal, i64, Decimal, Decimal);

/// ITEM-055 `(requests, ok, empty, refused, errors, truncated, guard_fallback,
/// unclassified)` — the overall quality totals row.
type QualityTotalsRow = (i64, i64, i64, i64, i64, i64, i64, i64);

/// `(provider_id, name, kind, requests, prompt_tokens, completion_tokens,
/// cost, limit_cents)` — one spend-by-provider row.
type ProviderAggregateRow = (
    Option<uuid::Uuid>,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    Decimal,
    i64,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
);

/// `(api_key_id, label, requests, prompt_tokens, completion_tokens, cost,
/// limit_cents)` — one spend-by-key row.
type KeyAggregateRow = (Option<uuid::Uuid>, String, i64, i64, i64, Decimal, i64);

/// `(date, requests, errors, cost, prompt_tokens, completion_tokens, avg_ttft,
/// avg_duration)` — one daily bucket.
type TimeseriesRow = (String, i64, i64, Decimal, i64, i64, Option<f64>, Option<f64>);

/// Shared aggregate query plumbing. `tenant` is `Some(id)` for tenant-scoped
/// views and `None` for the operator's platform-wide views (ITEM-013).
/// The SQL uses `($1::uuid IS NULL OR tenant_id = $1)` so one query shape
/// serves both scopes; the WHERE filter is the only separation.
///
/// Turn a storage error into the dashboard's 500 (`?`-friendly).
pub(crate) fn stored<T>(result: Result<T, sqlx::Error>) -> Result<T, Response> {
    result.map_err(db_err)
}

fn empty_summary() -> SummaryRow {
    (
        0,
        Decimal::ZERO,
        0,
        0,
        0,
        None,
        None,
        None,
        None,
        0,
        Decimal::ZERO,
        0,
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
        0,
    )
}

async fn summary_totals(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    days: i64,
    offset: i64,
) -> Result<SummaryRow, Response> {
    let row: Option<SummaryRow> = stored(
        sqlx::query_as(
            "SELECT COUNT(*),
                    COALESCE(SUM(upstream_cost_usd), 0),
                    COUNT(*) FILTER (WHERE status_code >= 400),
                    COALESCE(SUM(prompt_tokens), 0),
                    COALESCE(SUM(completion_tokens), 0),
                    AVG(time_to_first_token_ms)::float8,
                    AVG(total_duration_ms)::float8,
                    MIN(created_at)::text,
                    MAX(created_at)::text,
                    COALESCE(SUM(cached_prompt_tokens), 0),
                    COALESCE(SUM(cache_savings_usd), 0),
                    COUNT(*) FILTER (WHERE truncated),
                    COALESCE(SUM(savings_usd), 0),
                    COALESCE(SUM(savings_usd) FILTER (WHERE savings_kind = 'router'), 0),
                    COALESCE(SUM(savings_usd) FILTER (WHERE savings_kind = 'fallback'), 0),
                    COUNT(*) FILTER (WHERE cache_affinity IS NOT NULL)
             FROM traces
             WHERE ($1::uuid IS NULL OR tenant_id = $1)
               AND created_at >= NOW() - (($2 + $3) * INTERVAL '1 day')
               AND created_at < NOW() - ($3 * INTERVAL '1 day')",
        )
        .bind(tenant)
        .bind(days)
        .bind(offset)
        .fetch_optional(&state.db)
        .await,
    )?;
    Ok(row.unwrap_or_else(empty_summary))
}

async fn summary_latency(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    days: i64,
    offset: i64,
) -> Result<LatencyRow, Response> {
    stored(
        sqlx::query_as(
            "SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY time_to_first_token_ms)::float8,
                percentile_cont(0.95) WITHIN GROUP (ORDER BY time_to_first_token_ms)::float8,
                percentile_cont(0.99) WITHIN GROUP (ORDER BY time_to_first_token_ms)::float8,
                percentile_cont(0.5) WITHIN GROUP (ORDER BY total_duration_ms)::float8,
                percentile_cont(0.95) WITHIN GROUP (ORDER BY total_duration_ms)::float8,
                percentile_cont(0.99) WITHIN GROUP (ORDER BY total_duration_ms)::float8,
                percentile_cont(0.5) WITHIN GROUP (ORDER BY proxy_ms)::float8,
                percentile_cont(0.95) WITHIN GROUP (ORDER BY proxy_ms)::float8
         FROM traces
         WHERE ($1::uuid IS NULL OR tenant_id = $1)
           AND created_at >= NOW() - (($2 + $3) * INTERVAL '1 day')
           AND created_at < NOW() - ($3 * INTERVAL '1 day')",
        )
        .bind(tenant)
        .bind(days)
        .bind(offset)
        .fetch_one(&state.db)
        .await,
    )
}

/// ITEM-057: the failure taxonomy, grouped by class (null = no failure).
async fn error_classes(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    days: i64,
    offset: i64,
) -> Result<serde_json::Map<String, Value>, Response> {
    let rows: Vec<(String, i64)> = stored(
        sqlx::query_as(
            "SELECT error_class, COUNT(*)
             FROM traces
             WHERE error_class IS NOT NULL
               AND ($1::uuid IS NULL OR tenant_id = $1)
               AND created_at >= NOW() - (($2 + $3) * INTERVAL '1 day')
               AND created_at < NOW() - ($3 * INTERVAL '1 day')
             GROUP BY error_class ORDER BY 2 DESC",
        )
        .bind(tenant)
        .bind(days)
        .bind(offset)
        .fetch_all(&state.db)
        .await,
    )?;
    Ok(rows.into_iter().map(|(class, count)| (class, json!(count))).collect())
}

/// Unit economics computed server-side so no float ever touches a cost.
fn average_cost(total: Decimal, requests: i64) -> Decimal {
    if requests > 0 {
        (total / Decimal::from(requests)).round_dp(8)
    } else {
        Decimal::ZERO
    }
}

fn cache_hit_rate(cached_tokens: i64, prompt_tokens: i64) -> Option<f64> {
    (prompt_tokens > 0).then(|| cached_tokens as f64 / prompt_tokens as f64)
}

pub(crate) async fn query_summary(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
    scope_label: &str,
) -> Result<Response, Response> {
    let days = window.days();
    let offset = window.offset();
    let (
        requests,
        total_cost,
        errors,
        prompt_tokens,
        completion_tokens,
        avg_ttft_ms,
        avg_duration_ms,
        first_at,
        last_at,
        cached_tokens,
        cache_savings,
        truncated,
        savings,
        router_savings,
        fallback_savings,
        cache_affinity_requests,
    ) = summary_totals(state, tenant, days, offset).await?;
    // ITEM-056: tail latency percentiles (NULL when there are no samples).
    let (
        ttft_p50_ms,
        ttft_p95_ms,
        ttft_p99_ms,
        duration_p50_ms,
        duration_p95_ms,
        duration_p99_ms,
        proxy_p50_ms,
        proxy_p95_ms,
    ) = summary_latency(state, tenant, days, offset).await?;
    let errors_by_class = error_classes(state, tenant, days, offset).await?;
    let avg_cost = average_cost(total_cost, requests);
    let cache_hit_rate = cache_hit_rate(cached_tokens, prompt_tokens);
    Ok(json_ok(json!({
        "scope": scope_label,
        "tenant_id": tenant.map(|t| t.to_string()),
        "window_days": days,
        "offset_days": offset,
        "requests": requests,
        "errors": errors,
        "total_cost_usd": total_cost.to_string(),
        "avg_cost_usd": avg_cost.to_string(),
        "total_prompt_tokens": prompt_tokens,
        "total_completion_tokens": completion_tokens,
        "avg_ttft_ms": avg_ttft_ms,
        "avg_duration_ms": avg_duration_ms,
        "ttft_p50_ms": ttft_p50_ms,
        "ttft_p95_ms": ttft_p95_ms,
        "ttft_p99_ms": ttft_p99_ms,
        "duration_p50_ms": duration_p50_ms,
        "duration_p95_ms": duration_p95_ms,
        "duration_p99_ms": duration_p99_ms,
        "proxy_p50_ms": proxy_p50_ms,
        "proxy_p95_ms": proxy_p95_ms,
        "first_trace_at": first_at,
        "last_trace_at": last_at,
        "cached_prompt_tokens": cached_tokens,
        "cache_hit_rate": cache_hit_rate,
        "cache_savings_usd": cache_savings.to_string(),
        "truncated": truncated,
        "truncated_rate": rate(truncated, requests),
        "errors_by_class": errors_by_class,
        "router_savings_usd": router_savings.to_string(),
        "fallback_savings_usd": fallback_savings.to_string(),
        "total_savings_usd": (savings + cache_savings).to_string(),
        // ITEM-063: how many requests cache affinity re-targeted.
        "cache_affinity_requests": cache_affinity_requests,
        // What the request would have cost on the model the client asked for
        // (model-substitution axis; cache is a separate axis above).
        "unreduced_cost_usd": (total_cost + savings).to_string(),
    })))
}

pub(crate) async fn query_by_model(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
) -> Result<Response, Response> {
    let rows: Vec<ModelAggregateRow> = stored(sqlx::query_as(
        "SELECT request_model, COUNT(*), COALESCE(SUM(prompt_tokens), 0),
                COALESCE(SUM(completion_tokens), 0), COALESCE(SUM(upstream_cost_usd), 0),
                COALESCE(SUM(cached_prompt_tokens), 0), COALESCE(SUM(cache_savings_usd), 0),
                COALESCE(SUM(savings_usd), 0)
         FROM traces
         WHERE ($1::uuid IS NULL OR tenant_id = $1)
           AND created_at >= NOW() - ($2 * INTERVAL '1 day')
         GROUP BY request_model ORDER BY 5 DESC",
    )
    .bind(tenant)
    .bind(window.days())
    .fetch_all(&state.db)
    .await)?;

    let models: Vec<Value> = rows
        .into_iter()
        .map(|(model, requests, prompt_tokens, completion_tokens, cost, cached_tokens, cache_savings, savings)| {
            json!({
                "model": model,
                "requests": requests,
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "cost_usd": cost.to_string(),
                "cached_prompt_tokens": cached_tokens,
                "cache_savings_usd": cache_savings.to_string(),
                "savings_usd": savings.to_string(),
                // ITEM-058: what this model's traffic would have cost on the
                // models the clients asked for (the value of the cheaper tier).
                "frontier_cost_usd": (cost + savings).to_string(),
            })
        })
        .collect();
    Ok(json_ok(json!({"models": models})))
}

pub(crate) async fn query_by_prompt(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
) -> Result<Response, Response> {
    let rows: Vec<(Option<String>, i64, i64, Decimal, i64, Decimal)> = stored(sqlx::query_as(
        "SELECT system_prompt_hash, COUNT(*), COALESCE(SUM(prompt_tokens), 0),
                COALESCE(SUM(upstream_cost_usd), 0),
                COALESCE(SUM(cached_prompt_tokens), 0), COALESCE(SUM(cache_savings_usd), 0)
         FROM traces
         WHERE ($1::uuid IS NULL OR tenant_id = $1)
           AND created_at >= NOW() - ($2 * INTERVAL '1 day')
         GROUP BY system_prompt_hash ORDER BY 4 DESC LIMIT $3",
    )
    .bind(tenant)
    .bind(window.days())
    .bind(window.limit())
    .fetch_all(&state.db)
    .await)?;

    let prompts: Vec<Value> = rows
        .into_iter()
        .map(|(hash, requests, prompt_tokens, cost, cached_tokens, cache_savings)| {
            json!({
                "system_prompt_hash": hash.unwrap_or_else(|| "(no system prompt)".to_string()),
                "requests": requests,
                "prompt_tokens": prompt_tokens,
                "cached_prompt_tokens": cached_tokens,
                "cache_savings_usd": cache_savings.to_string(),
                // ITEM-063: cache-hit rate per prompt cluster (the affinity group).
                "cache_hit_rate": rate(cached_tokens, prompt_tokens),
                "cost_usd": cost.to_string(),
            })
        })
        .collect();
    Ok(json_ok(json!({"prompts": prompts})))
}

/// ITEM-047: spend/usage grouped by provider over the window (offset shifts it
/// back for period comparison). A `NULL` provider_id is the shared platform key.
async fn query_by_provider(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
) -> Result<Response, Response> {
    let days = window.days();
    let offset = window.offset();
    let rows: Vec<ProviderAggregateRow> =
        stored(sqlx::query_as(
            "SELECT t.provider_id,
                    COALESCE(p.name, 'Shared platform key'),
                    COALESCE(p.kind, 'shared'),
                    COUNT(*),
                    COUNT(*) FILTER (WHERE t.status_code >= 400),
                    COALESCE(SUM(t.prompt_tokens), 0),
                    COALESCE(SUM(t.completion_tokens), 0),
                    COALESCE(SUM(t.upstream_cost_usd), 0),
                    COALESCE(p.spend_limit_cents, 0),
                    AVG(t.time_to_first_token_ms)::float8,
                    percentile_cont(0.95) WITHIN GROUP (ORDER BY t.time_to_first_token_ms)::float8,
                    AVG(t.total_duration_ms)::float8,
                    percentile_cont(0.95) WITHIN GROUP (ORDER BY t.total_duration_ms)::float8
             FROM traces t
             LEFT JOIN providers p ON p.id = t.provider_id
             WHERE ($1::uuid IS NULL OR t.tenant_id = $1)
               AND t.created_at >= NOW() - (($2 + $3) * INTERVAL '1 day')
               AND t.created_at < NOW() - ($3 * INTERVAL '1 day')
             GROUP BY t.provider_id, p.name, p.kind, p.spend_limit_cents
             ORDER BY 7 DESC",
        )
        .bind(tenant)
        .bind(days)
        .bind(offset)
        .fetch_all(&state.db)
        .await)?;

    let providers: Vec<Value> = rows
        .into_iter()
        .map(
            |(
                id,
                name,
                kind,
                requests,
                errors,
                prompt_tokens,
                completion_tokens,
                cost,
                limit_cents,
                avg_ttft_ms,
                ttft_p95_ms,
                avg_duration_ms,
                duration_p95_ms,
            )| {
                json!({
                    "provider_id": id.map(|v| v.to_string()),
                    "name": name,
                    "kind": kind,
                    "requests": requests,
                    "errors": errors,
                    "error_rate": rate(errors, requests),
                    "avg_ttft_ms": avg_ttft_ms,
                    "ttft_p95_ms": ttft_p95_ms,
                    "avg_duration_ms": avg_duration_ms,
                    "duration_p95_ms": duration_p95_ms,
                    "prompt_tokens": prompt_tokens,
                    "completion_tokens": completion_tokens,
                    "cost_usd": cost.to_string(),
                    "spend_limit_cents": limit_cents,
                })
            },
        )
        .collect();
    Ok(json_ok(json!({
        "window_days": days,
        "offset_days": offset,
        "providers": providers,
    })))
}

/// ITEM-047: spend/usage grouped by API key over the window. A `NULL`
/// api_key_id is a legacy credential minted before per-key attribution.
async fn query_by_key(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
) -> Result<Response, Response> {
    let days = window.days();
    let offset = window.offset();
    let rows: Vec<KeyAggregateRow> = stored(sqlx::query_as(
        "SELECT t.api_key_id,
                COALESCE(k.label, 'Legacy key'),
                COUNT(*),
                COALESCE(SUM(t.prompt_tokens), 0),
                COALESCE(SUM(t.completion_tokens), 0),
                COALESCE(SUM(t.upstream_cost_usd), 0),
                COALESCE(k.spend_limit_cents, 0)
         FROM traces t
         LEFT JOIN api_keys k ON k.id = t.api_key_id
         WHERE ($1::uuid IS NULL OR t.tenant_id = $1)
           AND t.created_at >= NOW() - (($2 + $3) * INTERVAL '1 day')
           AND t.created_at < NOW() - ($3 * INTERVAL '1 day')
         GROUP BY t.api_key_id, k.label, k.spend_limit_cents
         ORDER BY 6 DESC",
    )
    .bind(tenant)
    .bind(days)
    .bind(offset)
    .fetch_all(&state.db)
    .await)?;

    let keys: Vec<Value> = rows
        .into_iter()
        .map(
            |(id, label, requests, prompt_tokens, completion_tokens, cost, limit_cents)| {
                json!({
                    "key_id": id.map(|v| v.to_string()),
                    "label": label,
                    "requests": requests,
                    "prompt_tokens": prompt_tokens,
                    "completion_tokens": completion_tokens,
                    "cost_usd": cost.to_string(),
                    "spend_limit_cents": limit_cents,
                })
            },
        )
        .collect();
    Ok(json_ok(json!({
        "window_days": days,
        "offset_days": offset,
        "keys": keys,
    })))
}

async fn query_waste(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
) -> Result<Response, Response> {
    // Grouped by the model pair so the estimate uses each row's own suggestion
    // (`gpt-4o` -> mini, `deepseek-v4-pro` -> flash) rather than a fixed pair.
    let rows: Vec<(String, String, String, i64, i64, i64, Decimal)> = stored(sqlx::query_as(
        "SELECT flag, request_model,
                COALESCE(waste_detected->>'suggested_model', '') AS suggested,
                COUNT(*), COALESCE(SUM(prompt_tokens), 0),
                COALESCE(SUM(completion_tokens), 0), COALESCE(SUM(upstream_cost_usd), 0)
         FROM traces
         CROSS JOIN LATERAL (VALUES
            ('over_provisioned', waste_detected->>'over_provisioned'),
            ('cache_miss', waste_detected->>'cache_miss_system_prompt'),
            ('model_misuse', waste_detected->>'model_misuse')) AS f(flag, fired)
         WHERE ($1::uuid IS NULL OR tenant_id = $1)
           AND created_at >= NOW() - ($2 * INTERVAL '1 day') AND f.fired = 'true'
         GROUP BY flag, request_model, suggested",
    )
    .bind(tenant)
    .bind(window.days())
    .fetch_all(&state.db)
    .await)?;

    #[derive(Default)]
    struct Totals {
        requests: i64,
        spend: Decimal,
        savings: Decimal,
    }
    let mut totals: std::collections::BTreeMap<String, Totals> = std::collections::BTreeMap::new();
    for (flag, model, suggested, requests, prompt_tokens, completion_tokens, cost) in rows {
        // A cache-miss saving needs provider caching to be real; leave it at 0.
        let savings = if flag == "cache_miss" || suggested.is_empty() {
            Decimal::ZERO
        } else {
            cost::savings_vs(
                &model,
                &suggested,
                cost::tokens_as_i32(prompt_tokens),
                cost::tokens_as_i32(completion_tokens),
                0,
            )
        };
        let entry = totals.entry(flag).or_default();
        entry.requests += requests;
        entry.spend += cost;
        entry.savings += savings;
    }

    let mut out = json!({});
    for (flag, totals) in totals {
        out[flag] = json!({
            "requests": totals.requests,
            "spend_usd": totals.spend.to_string(),
            "savings_est_usd": totals.savings.to_string(),
        });
    }
    Ok(json_ok(out))
}

/// Daily buckets over the window, gap-filled with zero rows so the chart is
/// continuous. `offset` shifts the window back (period comparison).
async fn query_timeseries(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
) -> Result<Response, Response> {
    let days = window.days();
    let offset = window.offset();
    let rows: Vec<TimeseriesRow> =
        stored(sqlx::query_as(
            "SELECT to_char(b.day, 'YYYY-MM-DD'),
                    COUNT(t.id),
                    COUNT(t.id) FILTER (WHERE t.status_code >= 400),
                    COALESCE(SUM(t.upstream_cost_usd), 0),
                    COALESCE(SUM(t.prompt_tokens), 0),
                    COALESCE(SUM(t.completion_tokens), 0),
                    AVG(t.time_to_first_token_ms)::float8,
                    AVG(t.total_duration_ms)::float8
             FROM generate_series(
                 (NOW() - (($2 + $3 - 1) * INTERVAL '1 day'))::date,
                 (NOW() - ($3 * INTERVAL '1 day'))::date,
                 '1 day'
             ) AS b(day)
             LEFT JOIN traces t
               ON t.created_at::date = b.day::date
              AND ($1::uuid IS NULL OR t.tenant_id = $1)
             GROUP BY b.day ORDER BY b.day",
        )
        .bind(tenant)
        .bind(days)
        .bind(offset)
        .fetch_all(&state.db)
        .await)?;

    let points: Vec<Value> = rows
        .into_iter()
        .map(
            |(
                date,
                requests,
                errors,
                cost,
                prompt_tokens,
                completion_tokens,
                avg_ttft_ms,
                avg_duration_ms,
            )| {
                json!({
                    "date": date,
                    "requests": requests,
                    "errors": errors,
                    "cost_usd": cost.to_string(),
                    "prompt_tokens": prompt_tokens,
                    "completion_tokens": completion_tokens,
                    "avg_ttft_ms": avg_ttft_ms,
                    "avg_duration_ms": avg_duration_ms,
                })
            },
        )
        .collect();
    Ok(json_ok(json!({
        "window_days": days,
        "offset_days": offset,
        "points": points,
    })))
}

/// GET /v1/dashboard/api/quality?days=7 (tenant scope) — ITEM-055 quality view:
/// derived outcomes, guard-fallback rate, and the optional client thumbs, overall
/// and by model / system-prompt cluster.
pub async fn quality(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Result<Response, Response> {
    let id = tenant_or(&state, &headers).await?;
    query_quality(&state, Some(id), &window).await
}

/// A rate as a fraction in 0..1, or `None` when there is no denominator.
pub(crate) fn rate(part: i64, total: i64) -> Option<f64> {
    (total > 0).then(|| part as f64 / total as f64)
}

async fn quality_totals(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    days: i64,
) -> Result<QualityTotalsRow, Response> {
    let totals: Option<QualityTotalsRow> = stored(
        sqlx::query_as(
            "SELECT COUNT(*),
                COUNT(*) FILTER (WHERE outcome = 'ok'),
                COUNT(*) FILTER (WHERE outcome = 'empty'),
                COUNT(*) FILTER (WHERE outcome = 'refused'),
                COUNT(*) FILTER (WHERE outcome = 'error'),
                COUNT(*) FILTER (WHERE outcome = 'truncated'),
                COUNT(*) FILTER (WHERE guard_event->>'outcome' = 'fallback'),
                COUNT(*) FILTER (WHERE outcome IS NULL)
             FROM traces
             WHERE ($1::uuid IS NULL OR tenant_id = $1)
               AND created_at >= NOW() - ($2 * INTERVAL '1 day')",
        )
        .bind(tenant)
        .bind(days)
        .fetch_optional(&state.db)
        .await,
    )?;
    Ok(totals.unwrap_or_default())
}

async fn quality_thumbs(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    days: i64,
) -> Result<(i64, i64), Response> {
    stored(
        sqlx::query_as(
            "SELECT COUNT(*) FILTER (WHERE signal = 'up'),
                COUNT(*) FILTER (WHERE signal = 'down')
             FROM trace_quality
             WHERE ($1::uuid IS NULL OR tenant_id = $1)
               AND created_at >= NOW() - ($2 * INTERVAL '1 day')",
        )
        .bind(tenant)
        .bind(days)
        .fetch_one(&state.db)
        .await,
    )
}

async fn quality_models(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    days: i64,
) -> Result<Vec<Value>, Response> {
    let rows: Vec<(String, i64, i64, i64, i64, i64, i64)> = stored(
        sqlx::query_as(
            "SELECT t.request_model, COUNT(*),
                COUNT(*) FILTER (WHERE t.outcome = 'ok'),
                COUNT(*) FILTER (WHERE t.outcome = 'empty'),
                COUNT(*) FILTER (WHERE t.outcome = 'error'),
                COUNT(*) FILTER (WHERE q.signal = 'up'),
                COUNT(*) FILTER (WHERE q.signal = 'down')
             FROM traces t
             LEFT JOIN trace_quality q ON q.trace_id = t.id
             WHERE ($1::uuid IS NULL OR t.tenant_id = $1)
               AND t.created_at >= NOW() - ($2 * INTERVAL '1 day')
             GROUP BY t.request_model ORDER BY 2 DESC",
        )
        .bind(tenant)
        .bind(days)
        .fetch_all(&state.db)
        .await,
    )?;
    Ok(rows
        .into_iter()
        .map(|(model, requests, ok, empty, errors, up, down)| {
            json!({
                "model": model,
                "requests": requests,
                "ok": ok,
                "empty": empty,
                "errors": errors,
                "thumbs_up": up,
                "thumbs_down": down,
                "success_rate": rate(ok, requests),
            })
        })
        .collect())
}

async fn quality_prompts(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    days: i64,
) -> Result<Vec<Value>, Response> {
    let rows: Vec<(String, i64, i64, i64, i64)> = stored(
        sqlx::query_as(
            "SELECT system_prompt_hash, COUNT(*),
                COUNT(*) FILTER (WHERE outcome = 'ok'),
                COUNT(*) FILTER (WHERE outcome = 'empty'),
                COUNT(*) FILTER (WHERE outcome = 'error')
             FROM traces
             WHERE system_prompt_hash IS NOT NULL
               AND ($1::uuid IS NULL OR tenant_id = $1)
               AND created_at >= NOW() - ($2 * INTERVAL '1 day')
             GROUP BY system_prompt_hash ORDER BY 2 DESC LIMIT 20",
        )
        .bind(tenant)
        .bind(days)
        .fetch_all(&state.db)
        .await,
    )?;
    Ok(rows
        .into_iter()
        .map(|(hash, requests, ok, empty, errors)| {
            json!({
                "system_prompt_hash": hash,
                "requests": requests,
                "ok": ok,
                "empty": empty,
                "errors": errors,
                "success_rate": rate(ok, requests),
            })
        })
        .collect())
}

async fn query_quality(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
) -> Result<Response, Response> {
    let days = window.days();
    let (requests, ok, empty, refused, errors, truncated, guard_fallback, unclassified) =
        quality_totals(state, tenant, days).await?;
    let thumbs = quality_thumbs(state, tenant, days).await?;
    let model_rows = quality_models(state, tenant, days).await?;
    let prompt_rows = quality_prompts(state, tenant, days).await?;
    let errors_by_class = error_classes(state, tenant, days, 0).await?;

    Ok(json_ok(json!({
        "window_days": days,
        "scope": if tenant.is_some() { "tenant" } else { "operator" },
        "overall": {
            "requests": requests,
            "ok": ok,
            "empty": empty,
            "refused": refused,
            "errors": errors,
            "truncated": truncated,
            "unclassified": unclassified,
            "errors_by_class": errors_by_class,
            "guard_fallback": guard_fallback,
            "thumbs_up": thumbs.0,
            "thumbs_down": thumbs.1,
            "success_rate": rate(ok, requests),
            "empty_rate": rate(empty, requests),
            "refusal_rate": rate(refused, requests),
            "error_rate": rate(errors, requests),
            "guard_fallback_rate": rate(guard_fallback, requests),
            "truncated_rate": rate(truncated, requests),
        },
        "models": model_rows,
        "prompts": prompt_rows,
    })))
}

/// GET /v1/dashboard/api/summary?days=7 (tenant scope)
pub async fn summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Result<Response, Response> {
    let id = tenant_or(&state, &headers).await?;
    query_summary(&state, Some(id), &window, "tenant").await
}

/// GET /v1/dashboard/api/by-model?days=7 (tenant scope)
pub async fn by_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Result<Response, Response> {
    let id = tenant_or(&state, &headers).await?;
    query_by_model(&state, Some(id), &window).await
}

/// GET /v1/dashboard/api/by-prompt?days=7&limit=10 (tenant scope)
pub async fn by_prompt(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Result<Response, Response> {
    let id = tenant_or(&state, &headers).await?;
    query_by_prompt(&state, Some(id), &window).await
}

/// GET /v1/dashboard/api/by-provider?days=7 (tenant scope)
pub async fn by_provider(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Result<Response, Response> {
    let id = tenant_or(&state, &headers).await?;
    query_by_provider(&state, Some(id), &window).await
}

/// GET /v1/dashboard/api/by-key?days=7 (tenant scope)
pub async fn by_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Result<Response, Response> {
    let id = tenant_or(&state, &headers).await?;
    query_by_key(&state, Some(id), &window).await
}

/// GET /v1/dashboard/api/waste?days=7 (tenant scope)
pub async fn waste(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Result<Response, Response> {
    let id = tenant_or(&state, &headers).await?;
    query_waste(&state, Some(id), &window).await
}

/// GET /v1/dashboard/api/timeseries?days=30&offset=0 (tenant scope)
pub async fn timeseries(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Result<Response, Response> {
    let id = tenant_or(&state, &headers).await?;
    query_timeseries(&state, Some(id), &window).await
}

/// GET /v1/dashboard/api/guard?days=7 (tenant scope, ITEM-026 AC2)
pub async fn guard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Result<Response, Response> {
    let id = tenant_or(&state, &headers).await?;
    query_guard(&state, Some(id), &window, "tenant").await
}

/// Guard telemetry aggregate over the additive `guard_event` JSONB on traces
/// (ITEM-026 AC2/AC3). `tenant` is Some(id) for tenant scope, None for the
/// operator platform-wide view — same shape trick as the other queries.
/// Every event carries an outcome (flushed | fallback | fallback_failed) and,
/// on failures, the GuardReason label; counts break down per guarded model and
/// per failure reason, and fallbacking prompts (negative samples) come back in
/// the existing system-prompt-hash shape for later distillation harvests.
pub(crate) async fn query_guard(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
    scope_label: &str,
) -> Result<Response, Response> {
    let days = window.days();
    let row: Option<(i64, i64, i64)> = stored(sqlx::query_as(
        "SELECT COUNT(*),
                COUNT(*) FILTER (WHERE guard_event->>'outcome' = 'fallback'),
                COUNT(*) FILTER (WHERE guard_event->>'outcome' = 'fallback_failed')
         FROM traces
         WHERE ($1::uuid IS NULL OR tenant_id = $1)
           AND created_at >= NOW() - ($2 * INTERVAL '1 day')
           AND guard_event IS NOT NULL",
    )
    .bind(tenant)
    .bind(days)
    .fetch_optional(&state.db)
    .await)?;
    let (guarded, fallback, fallback_failed) = row.unwrap_or((0, 0, 0));

    let by_model: Vec<(String, i64, i64, i64)> = stored(sqlx::query_as(
        "SELECT COALESCE(guard_event->>'guarded_model', '(unknown)'), COUNT(*),
                COUNT(*) FILTER (WHERE guard_event->>'outcome' = 'fallback'),
                COUNT(*) FILTER (WHERE guard_event->>'outcome' = 'fallback_failed')
         FROM traces
         WHERE ($1::uuid IS NULL OR tenant_id = $1)
           AND created_at >= NOW() - ($2 * INTERVAL '1 day')
           AND guard_event IS NOT NULL
         GROUP BY 1 ORDER BY 2 DESC",
    )
    .bind(tenant)
    .bind(days)
    .fetch_all(&state.db)
    .await)?;
    let models: Vec<Value> = by_model
        .into_iter()
        .map(|(model, guarded, fallback, fallback_failed)| {
            json!({
                "guarded_model": model,
                "guarded": guarded,
                "fallback": fallback,
                "fallback_failed": fallback_failed,
            })
        })
        .collect();

    let by_reason: Vec<(String, i64)> = stored(sqlx::query_as(
        "SELECT guard_event->>'reason', COUNT(*)
         FROM traces
         WHERE ($1::uuid IS NULL OR tenant_id = $1)
           AND created_at >= NOW() - ($2 * INTERVAL '1 day')
           AND guard_event IS NOT NULL
           AND guard_event->>'reason' IS NOT NULL
         GROUP BY 1 ORDER BY 2 DESC",
    )
    .bind(tenant)
    .bind(days)
    .fetch_all(&state.db)
    .await)?;
    let reasons: Vec<Value> = by_reason
        .into_iter()
        .map(|(reason, events)| json!({ "reason": reason, "events": events }))
        .collect();

    let negative: Vec<(String, i64)> = stored(sqlx::query_as(
        "SELECT system_prompt_hash, COUNT(*)
         FROM traces
         WHERE ($1::uuid IS NULL OR tenant_id = $1)
           AND created_at >= NOW() - ($2 * INTERVAL '1 day')
           AND guard_event->>'outcome' = 'fallback'
           AND system_prompt_hash IS NOT NULL
         GROUP BY system_prompt_hash ORDER BY 2 DESC LIMIT $3",
    )
    .bind(tenant)
    .bind(days)
    .bind(window.limit())
    .fetch_all(&state.db)
    .await)?;
    let negative_samples: Vec<Value> = negative
        .into_iter()
        .map(|(hash, fallbacks)| json!({ "system_prompt_hash": hash, "fallbacks": fallbacks }))
        .collect();

    Ok(json_ok(json!({
        "scope": scope_label,
        "tenant_id": tenant.map(|t| t.to_string()),
        "window_days": days,
        "guarded_requests": guarded,
        "fallback_requests": fallback,
        "fallback_failed": fallback_failed,
        "by_model": models,
        "by_reason": reasons,
        "negative_samples": negative_samples,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn average_cost_is_zero_without_requests() {
        assert_eq!(average_cost(Decimal::from(5), 0), Decimal::ZERO);
    }

    #[test]
    fn average_cost_divides_and_rounds_to_eight_places() {
        assert_eq!(
            average_cost(Decimal::from(1), 3),
            Decimal::from_str_exact("0.33333333").unwrap()
        );
    }

    #[test]
    fn cache_hit_rate_is_none_without_prompt_tokens() {
        assert_eq!(cache_hit_rate(0, 0), None);
    }

    #[test]
    fn cache_hit_rate_is_the_cached_share() {
        assert_eq!(cache_hit_rate(250, 1_000), Some(0.25));
    }
}
