//! Transaction grouping (ITEM-093): the unit an agentic loop actually costs.
//!
//! A request may carry `X-Inferon-Session-Id`; this report groups by it and
//! answers "what does one user transaction cost", which per-request views
//! cannot. Requests without the header stay ungrouped and are not counted here.

use super::*;

/// `(session, turns, models, cost, savings, errors, first_seen, last_seen)`.
type SessionRow = (String, i64, i64, Decimal, Decimal, i64, String, String);

/// GET /v1/dashboard/api/sessions?days=30 — per-transaction cost, plus the
/// distribution across transactions.
pub async fn sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Result<Response, Response> {
    let (tenant_id, role) = session_membership(&state, &headers).await?;
    let days = window.days();

    let distribution: (i64, Option<f64>, Option<f64>) = stored(sqlx::query_as(
        "WITH per_session AS (
             SELECT session_id, SUM(upstream_cost_usd) AS cost
             FROM traces
             WHERE tenant_id = $1
               AND session_id IS NOT NULL
               AND created_at >= NOW() - ($2 * INTERVAL '1 day')
             GROUP BY session_id
         )
         SELECT COUNT(*),
                percentile_cont(0.5) WITHIN GROUP (ORDER BY cost)::float8,
                percentile_cont(0.95) WITHIN GROUP (ORDER BY cost)::float8
         FROM per_session",
    )
    .bind(tenant_id)
    .bind(days)
    .fetch_one(&state.db)
    .await)?;

    let rows: Vec<SessionRow> = stored(sqlx::query_as(
        "SELECT session_id,
                COUNT(*),
                COUNT(DISTINCT request_model),
                COALESCE(SUM(upstream_cost_usd), 0),
                COALESCE(SUM(savings_usd)
                    FILTER (WHERE savings_kind IN ('router', 'fallback')), 0),
                COUNT(*) FILTER (WHERE status_code >= 400),
                MIN(created_at)::text,
                MAX(created_at)::text
         FROM traces
         WHERE tenant_id = $1
           AND session_id IS NOT NULL
           AND created_at >= NOW() - ($2 * INTERVAL '1 day')
         GROUP BY session_id
         ORDER BY 4 DESC
         LIMIT 20",
    )
    .bind(tenant_id)
    .bind(days)
    .fetch_all(&state.db)
    .await)?;

    let (sessions, p50, p95) = distribution;
    Ok(json_ok(json!({
        "window": {"days": days},
        "role": role,
        "distribution": {
            "sessions": sessions,
            "p50_cost_usd": p50.map(|value| format!("{value:.6}")),
            "p95_cost_usd": p95.map(|value| format!("{value:.6}")),
        },
        "sessions": rows.into_iter().map(|(session, turns, models, cost, savings, errors, first, last)| json!({
            "session": session,
            "turns": turns,
            "models": models,
            "cost_usd": cost.to_string(),
            "savings_usd": savings.to_string(),
            "errors": errors,
            "first_seen": first,
            "last_seen": last,
        })).collect::<Vec<_>>(),
    })))
}
