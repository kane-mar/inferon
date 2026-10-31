//! Running an eval against the pinned gold set (ITEM-098 AC2/AC3).
//!
//! Each case is a real provider call with the pinned input. The call is traced
//! **into the eval tables only** — never `traces` — so production aggregates,
//! routing and the tenant's budget are untouched by construction (AC6).
//!
//! The run is bounded twice: by case count and by spend. When a bound is hit the
//! run is marked `partial`, never padded with zeros (AC5).

use serde_json::Value;
use uuid::Uuid;

use crate::errors::ErrorClass;
use crate::proxy::Surface;
use crate::quality::outcome_for_json;
use crate::{cost, AppState};

/// Default and maximum bounds for one run.
pub const DEFAULT_CASES: i64 = 20;
pub const MAX_CASES: i64 = 100;
pub const DEFAULT_COST_CENTS: i64 = 50;
pub const MAX_COST_CENTS: i64 = 5_000;

/// One pinned case.
#[derive(Clone, Debug)]
pub struct GoldCase {
    pub trace_id: Uuid,
    pub request: Value,
    pub reference: Option<Value>,
}

/// What happened to one case.
pub struct CaseResult {
    pub trace_id: Uuid,
    pub status: &'static str,
    pub method: Option<String>,
    pub score: Option<f64>,
    pub latency_ms: i64,
    pub cost_usd: rust_decimal::Decimal,
    pub detail: Option<String>,
}

/// Pin a gold set from the tenant's qualified examples.
///
/// One case per distinct prompt hash (the most recent qualified pair), so a
/// single chatty prompt cannot dominate the set. Returns the new set's id and
/// version, or `None` when there is nothing to pin.
pub async fn pin_gold_set(
    pool: &sqlx::PgPool,
    tenant_id: Uuid,
    cases: i64,
) -> Result<Option<(Uuid, i64, i64)>, sqlx::Error> {
    let limit = cases.clamp(1, MAX_CASES);
    let version: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM eval_gold_sets WHERE tenant_id = $1",
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?;

    let pinned: Option<(Uuid, i64)> = sqlx::query_as(
        "WITH picked AS (
             SELECT DISTINCT ON (t.system_prompt_hash)
                    t.id, t.system_prompt_hash, t.request_model, t.raw_prompt, t.raw_completion
             FROM traces t
             LEFT JOIN (SELECT trace_id FROM trace_quality WHERE signal = 'down') down
                    ON down.trace_id = t.id
             WHERE t.tenant_id = $1
               AND t.example_state = 'qualified'
               AND down.trace_id IS NULL
               AND t.raw_prompt IS NOT NULL
               AND t.raw_completion IS NOT NULL
             ORDER BY t.system_prompt_hash, t.created_at DESC
             LIMIT $2
         )
         INSERT INTO eval_gold_sets (tenant_id, version, cases)
         SELECT $1, $3, (SELECT COUNT(*) FROM picked)
         RETURNING id, (SELECT COUNT(*) FROM picked)",
    )
    .bind(tenant_id)
    .bind(limit)
    .bind(version)
    .fetch_optional(pool)
    .await?;

    let Some((gold_set_id, count)) = pinned else {
        return Ok(None);
    };
    if count == 0 {
        sqlx::query("DELETE FROM eval_gold_sets WHERE id = $1")
            .bind(gold_set_id)
            .execute(pool)
            .await?;
        return Ok(None);
    }

    sqlx::query(
        "INSERT INTO eval_gold_cases (gold_set_id, trace_id, prompt_hash, source_model, request, reference)
         SELECT $1, t.id, t.system_prompt_hash, t.request_model, t.raw_prompt, t.raw_completion
         FROM traces t
         LEFT JOIN (SELECT trace_id FROM trace_quality WHERE signal = 'down') down
                ON down.trace_id = t.id
         WHERE t.id IN (
             SELECT DISTINCT ON (t2.system_prompt_hash) t2.id
             FROM traces t2
             LEFT JOIN (SELECT trace_id FROM trace_quality WHERE signal = 'down') d2
                    ON d2.trace_id = t2.id
             WHERE t2.tenant_id = $2
               AND t2.example_state = 'qualified'
               AND d2.trace_id IS NULL
               AND t2.raw_prompt IS NOT NULL
               AND t2.raw_completion IS NOT NULL
             ORDER BY t2.system_prompt_hash, t2.created_at DESC
             LIMIT $3
         )",
    )
    .bind(gold_set_id)
    .bind(tenant_id)
    .bind(limit)
    .execute(pool)
    .await?;

    Ok(Some((gold_set_id, version, count)))
}

/// Load a gold set's cases, oldest pin first (stable order).
pub async fn gold_cases(pool: &sqlx::PgPool, gold_set_id: Uuid, limit: i64) -> Vec<GoldCase> {
    let rows: Result<Vec<(Uuid, Value, Option<Value>)>, sqlx::Error> = sqlx::query_as(
        "SELECT trace_id, request, reference FROM eval_gold_cases
         WHERE gold_set_id = $1 ORDER BY trace_id LIMIT $2",
    )
    .bind(gold_set_id)
    .bind(limit)
    .fetch_all(pool)
    .await;
    match rows {
        Ok(rows) => rows
            .into_iter()
            .map(|(trace_id, request, reference)| GoldCase { trace_id, request, reference })
            .collect(),
        Err(e) => {
            tracing::error!("eval: gold set lookup failed: {e}");
            Vec::new()
        }
    }
}

/// Execute one run. Cheap to spawn; the caller reports progress through the row.
pub async fn run(state: std::sync::Arc<AppState>, run_id: Uuid, gold_set_id: Uuid, model: String) {
    let pool = state.db.clone();
    let run: Option<(Uuid, i64, i64)> = sqlx::query_as(
        "SELECT tenant_id, max_cases, max_cost_cents FROM eval_runs WHERE id = $1",
    )
    .bind(run_id)
    .fetch_optional(&pool)
    .await
    .unwrap_or(None);
    let Some((tenant_id, max_cases, max_cost_cents)) = run else {
        tracing::error!("eval run {run_id} disappeared before it started");
        return;
    };

    let tenant = match state.tenant_store.tenant_by_id(tenant_id).await {
        Ok(Some(tenant)) => tenant,
        Ok(None) => {
            finish(&pool, run_id, Outcome::failed()).await;
            return;
        }
        Err(e) => {
            tracing::error!("eval: tenant lookup failed: {e}");
            finish(&pool, run_id, Outcome::failed()).await;
            return;
        }
    };
    let route = match state
        .hotpath
        .route(&state, &tenant, &model, Surface::OpenAi, None)
        .await
    {
        Ok(decision) => decision.route,
        Err(resp) => {
            tracing::warn!(
                "eval run {run_id}: no route for {model} ({})",
                resp.status()
            );
            finish(&pool, run_id, Outcome::failed()).await;
            return;
        }
    };

    let cases = gold_cases(&pool, gold_set_id, max_cases).await;
    let (mut scored, mut passed, mut failed) = (0i64, 0i64, 0i64);
    let mut spend = rust_decimal::Decimal::ZERO;
    let mut stopped_early = false;

    for case in &cases {
        if spend >= rust_decimal::Decimal::from(max_cost_cents) / rust_decimal::Decimal::from(100) {
            stopped_early = true;
            break;
        }
        let result = one_case(&state, &route, &model, case).await;
        spend += result.cost_usd;
        match result.status {
            "scored" => {
                scored += 1;
                if result.score.unwrap_or(0.0) >= 1.0 {
                    passed += 1;
                }
            }
            _ => failed += 1,
        }
        let stored = sqlx::query(
            "INSERT INTO eval_cases (run_id, trace_id, status, method, score, latency_ms, cost_usd, detail)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (run_id, trace_id) DO NOTHING",
        )
        .bind(run_id)
        .bind(case.trace_id)
        .bind(result.status)
        .bind(&result.method)
        .bind(result.score)
        .bind(result.latency_ms as i32)
        .bind(result.cost_usd)
        .bind(&result.detail)
        .execute(&pool)
        .await;
        if let Err(e) = stored {
            tracing::error!("eval: case store failed: {e}");
        }
    }

    let status = if failed > 0 || stopped_early || cases.len() as i64 > scored + failed {
        "partial"
    } else {
        "complete"
    };
    finish(
        &pool,
        run_id,
        Outcome {
            status,
            cases: cases.len() as i64,
            scored,
            passed,
            failed,
            spend,
        },
    )
    .await;
}

/// One provider call, scored. Never fails the run: an upstream error is a
/// recorded case failure with a reason, not a zero score.
async fn one_case(
    state: &AppState,
    route: &crate::proxy::Route,
    model: &str,
    case: &GoldCase,
) -> CaseResult {
    let request = match budget_body(&case.request, model) {
        Ok(body) => body,
        Err(detail) => {
            return failed_case(case.trace_id, "request_unsupported", detail);
        }
    };
    let started = std::time::Instant::now();
    let sent = crate::proxy::provider_request(&state.http_client, route)
        .body(request)
        .send()
        .await;
    let latency_ms = started.elapsed().as_millis() as i64;
    let response = match sent {
        Ok(response) => response,
        Err(e) => return failed_case(case.trace_id, "transport", e.to_string()),
    };
    let status = response.status();
    let body: Value = match response.json().await {
        Ok(body) => body,
        Err(e) => return failed_case(case.trace_id, "upstream_body", e.to_string()),
    };
    let parsed = Some(body.clone());
    let outcome = outcome_for_json(status.as_u16(), &parsed);
    let usage = crate::usage::usage_from_json(&parsed);
    let cost = cost::calculate_cost(model, usage.prompt_tokens, usage.completion_tokens, 0);
    if !status.is_success() {
        let mut result = failed_case(
            case.trace_id,
            "upstream_error",
            ErrorClass::from_upstream_status(status.as_u16())
                .as_str()
                .to_string(),
        );
        result.cost_usd = cost;
        result.latency_ms = latency_ms;
        return result;
    }

    let answer = crate::eval::answer_text(&body);
    let reference = case
        .reference
        .as_ref()
        .map(crate::eval::answer_text)
        .filter(|text| !text.is_empty());
    let schema = crate::guard::request_schema(&case.request);
    let graded = crate::eval::score(reference.as_deref(), schema.as_ref(), &answer, outcome);
    CaseResult {
        trace_id: case.trace_id,
        status: "scored",
        method: Some(graded.method.as_str().to_string()),
        score: Some(if graded.pass { 1.0 } else { 0.0 }),
        latency_ms,
        cost_usd: cost,
        detail: None,
    }
}

fn failed_case(trace_id: Uuid, status: &'static str, detail: String) -> CaseResult {
    CaseResult {
        trace_id,
        status,
        method: None,
        score: None,
        latency_ms: 0,
        cost_usd: rust_decimal::Decimal::ZERO,
        detail: Some(format!("{status}: {detail}")),
    }
}

/// The wire body: the pinned request with the eval model and streaming off.
fn budget_body(request: &Value, model: &str) -> Result<Vec<u8>, String> {
    let mut request = request.clone();
    request["model"] = Value::String(model.to_string());
    request["stream"] = Value::Bool(false);
    serde_json::to_vec(&request).map_err(|e| e.to_string())
}

/// The final shape of a run, written once.
struct Outcome {
    status: &'static str,
    cases: i64,
    scored: i64,
    passed: i64,
    failed: i64,
    spend: rust_decimal::Decimal,
}

impl Outcome {
    fn failed() -> Self {
        Self {
            status: "failed",
            cases: 0,
            scored: 0,
            passed: 0,
            failed: 0,
            spend: rust_decimal::Decimal::ZERO,
        }
    }
}

async fn finish(pool: &sqlx::PgPool, run_id: Uuid, outcome: Outcome) {
    let updated = sqlx::query(
        "UPDATE eval_runs SET status = $2, cases = $3, scored = $4, passed = $5,
                failed = $6, cost_usd = $7, finished_at = NOW()
         WHERE id = $1",
    )
    .bind(run_id)
    .bind(outcome.status)
    .bind(outcome.cases)
    .bind(outcome.scored)
    .bind(outcome.passed)
    .bind(outcome.failed)
    .bind(outcome.spend)
    .execute(pool)
    .await;
    if let Err(e) = updated {
        tracing::error!("eval: run update failed: {e}");
    }
}

#[cfg(test)]
mod tests;
