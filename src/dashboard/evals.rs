// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! The eval surface (ITEM-098): pin a gold set, run a model against it, compare
//! two runs.
//!
//! Owner-only, bounded, and never part of the production read models: eval calls
//! are recorded in the `eval_*` tables and never in `traces`, so they cannot
//! move a tenant's spend, routing or policy.

use uuid::Uuid;

use super::*;

/// `(id, model, status, cases, scored, passed, failed, cost, created_at)`.
type RunRow = (Uuid, String, String, i64, i64, i64, i64, Decimal, String);

/// POST /v1/dashboard/api/evals/gold — pin a gold set from verified examples.
pub async fn pin_gold(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<GoldBody>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    let cases = body.cases.unwrap_or(crate::eval::runner::DEFAULT_CASES);
    match crate::eval::runner::pin_gold_set(&state.db, tenant_id, cases).await {
        Ok(Some((id, version, count))) => json_ok(json!({
            "gold_set_id": id.to_string(),
            "version": version,
            "cases": count,
        })),
        Ok(None) => crate::json_error(
            StatusCode::BAD_REQUEST,
            "no_verified_examples",
            "no verified examples to pin yet; the dataset surface shows what qualifies",
        ),
        Err(e) => internal(e.to_string()),
    }
}

/// POST /v1/dashboard/api/evals/run — start a run and return immediately.
///
/// The run is bounded by `max_cases` and `max_cost_cents`; progress is polled
/// through `GET /v1/dashboard/api/evals`.
pub async fn start_run(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<RunBody>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    let Some(model) = body.model.filter(|model| !model.trim().is_empty()) else {
        return crate::json_error(StatusCode::BAD_REQUEST, "missing_model", "pass the model to evaluate");
    };
    let gold_set_id = match body.gold_set_id {
        Some(id) => id,
        None => match latest_gold_set(&state, tenant_id).await {
            Ok(Some(id)) => id,
            Ok(None) => {
                return crate::json_error(
                    StatusCode::BAD_REQUEST,
                    "no_gold_set",
                    "pin a gold set first (POST /evals/gold)",
                )
            }
            Err(e) => return internal(e.to_string()),
        },
    };
    let max_cases = body
        .max_cases
        .unwrap_or(crate::eval::runner::DEFAULT_CASES)
        .clamp(1, crate::eval::runner::MAX_CASES);
    let max_cost_cents = body
        .max_cost_cents
        .unwrap_or(crate::eval::runner::DEFAULT_COST_CENTS)
        .clamp(1, crate::eval::runner::MAX_COST_CENTS);

    let run_id: Uuid = match sqlx::query_scalar(
        "INSERT INTO eval_runs (tenant_id, gold_set_id, model, status, max_cases, max_cost_cents)
         VALUES ($1, $2, $3, 'running', $4, $5) RETURNING id",
    )
    .bind(tenant_id)
    .bind(gold_set_id)
    .bind(&model)
    .bind(max_cases)
    .bind(max_cost_cents)
    .fetch_one(&state.db)
    .await
    {
        Ok(id) => id,
        Err(e) => return internal(e.to_string()),
    };

    // Fire and forget: the run reports its own status. Eval calls never touch the
    // trace path, so this cannot move the tenant's spend or routing.
    let task_state = state.clone();
    let task_model = model.clone();
    tokio::spawn(async move {
        crate::eval::runner::run(task_state, run_id, gold_set_id, task_model).await;
    });

    json_ok(json!({
        "run_id": run_id.to_string(),
        "status": "running",
        "max_cases": max_cases,
        "max_cost_cents": max_cost_cents,
    }))
}

/// GET /v1/dashboard/api/evals — the gold sets and runs, newest first.
pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let (tenant_id, role) = session_membership(&state, &headers).await?;

    let gold: Vec<(Uuid, i64, i64, String)> = stored(sqlx::query_as(
        "SELECT id, version, cases, created_at::text FROM eval_gold_sets
         WHERE tenant_id = $1 ORDER BY version DESC LIMIT 20",
    )
    .bind(tenant_id)
    .fetch_all(&state.db)
    .await)?;

    let runs: Vec<RunRow> = stored(
        sqlx::query_as(
            "SELECT id, model, status, cases, scored, passed, failed, cost_usd, created_at::text
             FROM eval_runs WHERE tenant_id = $1 ORDER BY created_at DESC LIMIT 20",
        )
        .bind(tenant_id)
        .fetch_all(&state.db)
        .await,
    )?;

    Ok(json_ok(json!({
        "role": role,
        "gold_sets": gold.into_iter().map(|(id, version, cases, created)| json!({
            "gold_set_id": id.to_string(),
            "version": version,
            "cases": cases,
            "created_at": created,
        })).collect::<Vec<_>>(),
        "runs": runs.into_iter().map(|(id, model, status, cases, scored, passed, failed, cost, created)| json!({
            "run_id": id.to_string(),
            "model": model,
            "status": status,
            "cases": cases,
            "scored": scored,
            "passed": passed,
            "failed": failed,
            "pass_rate": (scored > 0).then(|| passed as f64 / scored as f64),
            "cost_usd": cost.to_string(),
            "created_at": created,
        })).collect::<Vec<_>>(),
    })))
}

/// POST /v1/dashboard/api/evals/compare — the regression gate for two runs.
pub async fn compare(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<CompareBody>,
) -> Result<Response, Response> {
    let (tenant_id, _role) = session_membership(&state, &headers).await?;
    let baseline = load_run(&state, tenant_id, body.baseline).await?;
    let challenger = load_run(&state, tenant_id, body.challenger).await?;
    let verdict = crate::eval::compare(&baseline, &challenger);
    Ok(json_ok(json!({
        "verdict": verdict.as_str(),
        "detail": match &verdict {
            crate::eval::Verdict::Insufficient { reason } => json!(reason),
            crate::eval::Verdict::Unchanged { .. }
            | crate::eval::Verdict::Improved { .. }
            | crate::eval::Verdict::Regressed { .. } => json!(format!(
                "{:.1}% vs {:.1}%",
                verdict_rate(&verdict).0 * 100.0,
                verdict_rate(&verdict).1 * 100.0,
            )),
        },
        "baseline": { "model": baseline.model, "scored": baseline.scored, "passed": baseline.passed },
        "challenger": { "model": challenger.model, "scored": challenger.scored, "passed": challenger.passed },
    })))
}

fn verdict_rate(verdict: &crate::eval::Verdict) -> (f64, f64) {
    match verdict {
        crate::eval::Verdict::Insufficient { .. } => (0.0, 0.0),
        crate::eval::Verdict::Unchanged { baseline, challenger }
        | crate::eval::Verdict::Improved { baseline, challenger }
        | crate::eval::Verdict::Regressed { baseline, challenger } => (*baseline, *challenger),
    }
}

async fn latest_gold_set(state: &AppState, tenant_id: Uuid) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM eval_gold_sets WHERE tenant_id = $1 ORDER BY version DESC LIMIT 1",
    )
    .bind(tenant_id)
    .fetch_optional(&state.db)
    .await
}

async fn load_run(
    state: &AppState,
    tenant_id: Uuid,
    run_id: Uuid,
) -> Result<crate::eval::RunScore, Response> {
    let row: Option<(String, i64, i64, i64)> = stored(
        sqlx::query_as(
            "SELECT model, scored, passed, failed FROM eval_runs
             WHERE id = $1 AND tenant_id = $2",
        )
        .bind(run_id)
        .bind(tenant_id)
        .fetch_optional(&state.db)
        .await,
    )?;
    match row {
        Some((model, scored, passed, failed)) => Ok(crate::eval::RunScore {
            run_id,
            model,
            scored,
            passed,
            failed,
        }),
        None => Err(crate::json_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "no such eval run",
        )),
    }
}

/// ITEM-098: pin a gold set (default: the runner's default case count).
#[derive(serde::Deserialize)]
pub struct GoldBody {
    #[serde(default)]
    cases: Option<i64>,
}

/// ITEM-098: start an eval run.
#[derive(serde::Deserialize)]
pub struct RunBody {
    model: Option<String>,
    #[serde(default)]
    gold_set_id: Option<uuid::Uuid>,
    #[serde(default)]
    max_cases: Option<i64>,
    #[serde(default)]
    max_cost_cents: Option<i64>,
}

/// ITEM-098: the regression gate for two runs.
#[derive(serde::Deserialize)]
pub struct CompareBody {
    baseline: uuid::Uuid,
    challenger: uuid::Uuid,
}

