//! The dataset report (ITEM-086): verified examples per prompt and model.
//!
//! Counts, not payloads: the read model answers "how many usable pairs did this
//! prompt produce, what did it cost, and why did the rest not qualify". Export
//! is ITEM-087.

use axum::body::Body;
use axum::http::header;
use futures_util::TryStreamExt;
use uuid::Uuid;

use super::*;

/// `(prompt_hash, model, requests, qualified, distinct, excluded, spend, last_seen)`.
type ClusterRow = (
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    Decimal,
    String,
);

/// `(prompt_hash, model, reason, count)`.
type ReasonRow = (String, String, String, i64);

/// GET /v1/dashboard/api/datasets?days=30 — prompt clusters ranked by the number
/// of distinct verified examples they produced.
pub async fn datasets(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Result<Response, Response> {
    let (tenant_id, role) = session_membership(&state, &headers).await?;

    let rows: Vec<ClusterRow> = stored(sqlx::query_as(
        "SELECT t.system_prompt_hash, t.request_model,
                COUNT(*),
                COUNT(*) FILTER (WHERE t.example_state = 'qualified' AND down.trace_id IS NULL),
                COUNT(DISTINCT t.completion_hash)
                    FILTER (WHERE t.example_state = 'qualified' AND down.trace_id IS NULL),
                COUNT(*) FILTER (WHERE t.example_state = 'excluded'),
                COALESCE(SUM(t.upstream_cost_usd), 0),
                MAX(t.created_at)::text
         FROM traces t
         LEFT JOIN (SELECT trace_id FROM trace_quality WHERE signal = 'down') down
                ON down.trace_id = t.id
         WHERE t.tenant_id = $1
           AND t.system_prompt_hash IS NOT NULL
           AND t.created_at >= NOW() - ($2 * INTERVAL '1 day')
         GROUP BY t.system_prompt_hash, t.request_model
         ORDER BY 5 DESC
         LIMIT 50",
    )
    .bind(tenant_id)
    .bind(window.days())
    .fetch_all(&state.db)
    .await)?;

    let reasons: Vec<ReasonRow> = stored(sqlx::query_as(
        "SELECT system_prompt_hash, request_model, example_reason, COUNT(*)
         FROM traces
         WHERE tenant_id = $1
           AND example_state = 'excluded'
           AND created_at >= NOW() - ($2 * INTERVAL '1 day')
         GROUP BY system_prompt_hash, request_model, example_reason",
    )
    .bind(tenant_id)
    .bind(window.days())
    .fetch_all(&state.db)
    .await)?;

    let mut by_reason: std::collections::HashMap<(String, String), serde_json::Map<String, Value>> =
        std::collections::HashMap::new();
    for (hash, model, reason, count) in reasons {
        by_reason
            .entry((hash, model))
            .or_default()
            .insert(reason, json!(count));
    }

    let clusters: Vec<Value> = rows
        .into_iter()
        .map(|(hash, model, requests, qualified, distinct, excluded, spend, last_seen)| {
            let excluded_by_reason = by_reason
                .remove(&(hash.clone(), model.clone()))
                .unwrap_or_default();
            json!({
                "prompt_hash": hash,
                "model": model,
                "requests": requests,
                "qualified": qualified,
                "distinct_examples": distinct,
                "excluded": excluded,
                "excluded_by_reason": excluded_by_reason,
                "spend_usd": spend.to_string(),
                "last_seen": last_seen,
            })
        })
        .collect();

    Ok(json_ok(json!({
        "window": {"days": window.days()},
        "role": role,
        "clusters": clusters,
    })))
}

/// POST /v1/dashboard/api/datasets/purge (owner-only) — delete every stored
/// payload for the tenant now (ITEM-088). Costs and aggregates survive.
pub async fn purge(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Result<Response, Response> {
    let (tenant_id, role) = session_membership(&state, &headers).await?;
    owner_only(&role)?;
    match crate::retention::purge_tenant(&state.db, tenant_id).await {
        Ok(purged) => {
            tracing::info!("payload purge: tenant {tenant_id} cleared {purged} row(s)");
            Ok(json_ok(json!({ "purged": purged })))
        }
        Err(e) => Err(internal(e.to_string())),
    }
}

/// Query for one cluster's export.
#[derive(serde::Deserialize)]
pub struct ExportQuery {
    prompt_hash: String,
    model: String,
    #[serde(default)]
    days: Option<i64>,
}

/// One JSONL line: the prompt's messages plus the assistant turn, with
/// provenance. `None` when either side is not a usable pair.
pub(crate) fn jsonl_line(prompt: &Value, completion: &Value, model: &str, trace_id: Uuid) -> Option<String> {
    let mut messages = prompt["messages"].as_array()?.clone();
    let assistant = completion["choices"][0]["message"].clone();
    if !assistant.is_object() {
        return None;
    }
    messages.push(assistant);
    Some(
        json!({
            "messages": messages,
            "model": model,
            "trace_id": trace_id.to_string(),
        })
        .to_string(),
    )
}

/// GET /v1/dashboard/api/datasets/export?prompt_hash=..&model=.. — stream one
/// cluster's verified examples as JSONL (ITEM-087). Owner-only, and refused
/// when the tenant does not capture payloads.
pub async fn export(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ExportQuery>,
) -> Result<Response, Response> {
    let (tenant_id, role) = session_membership(&state, &headers).await?;
    owner_only(&role)?;
    if !state.hotpath.capture_enabled(&state, tenant_id).await {
        return Err(crate::json_error(
            StatusCode::BAD_REQUEST,
            "capture_disabled",
            "this tenant does not capture payloads, so there is nothing to export",
        ));
    }
    let days = query.days.unwrap_or(30).clamp(1, 3650);

    let prompt_hash = query.prompt_hash.clone();
    let model = query.model.clone();
    let filename = format!(
        "inferon-{}-{}.jsonl",
        &query.prompt_hash[..query.prompt_hash.len().min(8)],
        query.model.replace(['/', ':'], "-")
    );

    let pool = state.db.clone();
    let body = async_stream::stream! {
        let mut written = 0u64;
        let mut rows = sqlx::query_as::<_, (Value, Value, Uuid)>(
            "SELECT DISTINCT ON (t.completion_hash) t.raw_prompt, t.raw_completion, t.id
             FROM traces t
             LEFT JOIN (SELECT trace_id FROM trace_quality WHERE signal = 'down') down
                    ON down.trace_id = t.id
             WHERE t.tenant_id = $1
               AND t.system_prompt_hash = $2
               AND t.request_model = $3
               AND t.example_state = 'qualified'
               AND down.trace_id IS NULL
               AND t.raw_prompt IS NOT NULL
               AND t.raw_completion IS NOT NULL
               AND t.created_at >= NOW() - ($4 * INTERVAL '1 day')
             ORDER BY t.completion_hash, t.created_at
             LIMIT 10000",
        )
        .bind(tenant_id)
        .bind(prompt_hash.clone())
        .bind(model.clone())
        .bind(days)
        .fetch(&pool);
        while let Some(row) = rows.try_next().await.transpose() {
            match row {
                Ok((prompt, completion, trace_id)) => {
                    if let Some(line) = jsonl_line(&prompt, &completion, &model, trace_id) {
                        written += 1;
                        yield Ok::<_, std::io::Error>(axum::body::Bytes::from(format!("{line}\n")));
                    }
                }
                Err(e) => {
                    tracing::error!("dataset export read failed: {e}");
                    break;
                }
            }
        }
        tracing::info!(
            "dataset export: tenant {tenant_id} prompt {prompt_hash} model {model} rows {written}"
        );
    };

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\""))
        .body(Body::from_stream(body))
        .expect("valid response"))
}

#[cfg(test)]
mod tests;
