//! The dataset report (ITEM-086): verified examples per prompt and model.
//!
//! Counts, not payloads: the read model answers "how many usable pairs did this
//! prompt produce, what did it cost, and why did the rest not qualify". Export
//! is ITEM-087.

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
    let (tenant_id, _role) = session_membership(&state, &headers).await?;

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
        "clusters": clusters,
    })))
}
