//! Cost-leak dashboard (ITEM-007): read-only aggregate views over `traces`,
//! always scoped to the authenticated tenant (AC5). No new write path (AC3).
//!
//! Data views are JSON APIs consumed by a small self-contained HTML page;
//! charts are CSS bars so there is no external asset or CDN dependency.
//! The operator console (ITEM-037) is the one server-rendered page.

use crate::cost;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::sync::Arc;

/// Query params shared by the JSON APIs: `days` (window, default 7, clamp
/// 1..=365) and `limit` (row cap for by-prompt, default 10).
#[derive(Debug, serde::Deserialize)]
pub struct Window {
    days: Option<i64>,
    limit: Option<i64>,
}

impl Window {
    fn days(&self) -> i64 {
        self.days.unwrap_or(7).clamp(1, 365)
    }
    fn limit(&self) -> i64 {
        self.limit.unwrap_or(10).clamp(1, 100)
    }
}

fn unauthorized() -> Response {
    crate::json_error(
        StatusCode::UNAUTHORIZED,
        "invalid_api_key",
        "missing or invalid Authorization header",
    )
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn forbidden() -> Response {
    crate::json_error(
        StatusCode::FORBIDDEN,
        "forbidden",
        "this credential is not allowed here",
    )
}

fn internal(message: &str) -> Response {
    tracing::error!("dashboard query failed: {message}");
    crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, "dashboard_error", message)
}

fn json_ok(value: Value) -> Response {
    Json(value).into_response()
}

fn db_err(e: sqlx::Error) -> Response {
    internal(&e.to_string())
}

/// Scope of an authenticated dashboard caller.
#[derive(Debug, PartialEq, Eq)]
enum Scope {
    Tenant(uuid::Uuid),
    Operator,
}

/// Bearer-only resolution (ITEM-004/013 semantics, unchanged): operator key
/// first, then the tenant key lookup. `Ok(None)` when no usable credential was
/// presented — the caller may still fall through to the session path.
async fn bearer_scope(state: &Arc<AppState>, headers: &HeaderMap) -> Result<Option<Scope>, Response> {
    let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) else {
        return Ok(None);
    };
    let Some(key) = auth.strip_prefix("Bearer ") else {
        return Ok(None);
    };
    if key.is_empty() {
        return Ok(None);
    }
    if state.operator_key.as_deref() == Some(key) {
        return Ok(Some(Scope::Operator));
    }
    match crate::authenticate(state, headers).await {
        Ok(Some(tenant)) => Ok(Some(Scope::Tenant(tenant.id))),
        Ok(None) => Ok(None),
        Err(e) => {
            tracing::error!("tenant lookup failed: {e}");
            Err(internal("tenant lookup failed"))
        }
    }
}

/// Session-based resolution (ITEM-036 AC2): a signed-in owner/member reaches
/// their tenant scope through the session cookie — no API key needed. Missing,
/// forged, or expired cookies (or a memberless user) yield `Ok(None)`, never
/// an error.
async fn scope_from_session(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<Option<Scope>, Response> {
    let Some(accounts) = &state.accounts else {
        return Ok(None);
    };
    let Some(cookie) = headers.get("cookie").and_then(|v| v.to_str().ok()) else {
        return Ok(None);
    };
    let Some(token) = crate::session::cookie_value(cookie, crate::session::SESSION_COOKIE) else {
        return Ok(None);
    };
    let Some(user_id) =
        crate::session::verify_session(token, &accounts.session_secret, now_secs())
    else {
        return Ok(None);
    };
    let user = match accounts.users.find_by_id(user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => return Ok(None),
        Err(e) => {
            tracing::error!("user lookup failed: {e}");
            return Err(internal("user lookup failed"));
        }
    };
    // A platform operator's session scopes to the operator views (ITEM-037) —
    // and, like the operator key, is rejected from tenant-scoped routes.
    if user.is_operator {
        return Ok(Some(Scope::Operator));
    }
    match accounts.users.tenant_for_user(user.id).await {
        Ok(Some((tenant_id, _))) => Ok(Some(Scope::Tenant(tenant_id))),
        Ok(None) => Ok(None),
        Err(e) => {
            tracing::error!("membership lookup failed: {e}");
            Err(internal("membership lookup failed"))
        }
    }
}

/// Resolve the caller's scope: the bearer credential (operator key, then
/// tenant key — SDKs/scripts, ITEM-004) first, then the session cookie
/// (humans, ITEM-036). Additive: key auth is unchanged, sessions only add a
/// path.
async fn resolve_scope(state: &Arc<AppState>, headers: &HeaderMap) -> Result<Scope, Response> {
    if let Some(scope) = bearer_scope(state, headers).await? {
        return Ok(scope);
    }
    if let Some(scope) = scope_from_session(state, headers).await? {
        return Ok(scope);
    }
    Err(unauthorized())
}

/// Tenant-scoped handlers only: an operator credential is rejected (403).
async fn tenant_or(state: &Arc<AppState>, headers: &HeaderMap) -> Result<uuid::Uuid, Response> {
    match resolve_scope(state, headers).await? {
        Scope::Tenant(id) => Ok(id),
        Scope::Operator => Err(forbidden()),
    }
}

/// Operator-only handlers: tenant credentials are rejected (403). Returns the
/// tenant filter — `None` = platform-wide.
async fn operator_or(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<(), Response> {
    match resolve_scope(state, headers).await? {
        Scope::Operator => Ok(()),
        Scope::Tenant(_) => Err(forbidden()),
    }
}

/// The signed-in user's tenant membership — the human identity behind the
/// tenant-scoped management APIs (ITEM-038). Sessions only: a bare tenant key
/// is not a human identity (machines use the operator key API) and operator
/// sessions belong to the platform console, not a tenant role.
async fn session_membership(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<(uuid::Uuid, String), Response> {
    let Some(accounts) = &state.accounts else {
        return Err(unauthorized());
    };
    let Some(cookie) = headers.get("cookie").and_then(|v| v.to_str().ok()) else {
        return Err(unauthorized());
    };
    let Some(token) = crate::session::cookie_value(cookie, crate::session::SESSION_COOKIE) else {
        return Err(unauthorized());
    };
    let Some(user_id) =
        crate::session::verify_session(token, &accounts.session_secret, now_secs())
    else {
        return Err(unauthorized());
    };
    let user = match accounts.users.find_by_id(user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => return Err(unauthorized()),
        Err(e) => {
            tracing::error!("user lookup failed: {e}");
            return Err(internal("user lookup failed"));
        }
    };
    if user.is_operator {
        return Err(forbidden());
    }
    match accounts.users.tenant_for_user(user.id).await {
        Ok(Some(membership)) => Ok(membership),
        Ok(None) => Err(unauthorized()),
        Err(e) => {
            tracing::error!("membership lookup failed: {e}");
            Err(internal("membership lookup failed"))
        }
    }
}

/// Shared aggregate query plumbing. `tenant` is `Some(id)` for tenant-scoped
/// views and `None` for the operator's platform-wide views (ITEM-013).
/// The SQL uses `($1::uuid IS NULL OR tenant_id = $1)` so one query shape
/// serves both scopes; the WHERE filter is the only separation.
async fn query_summary(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
    scope_label: &str,
) -> Response {
    let days = window.days();
    let row: Option<(i64, Decimal, i64, Option<String>, Option<String>)> =
        match sqlx::query_as(
            "SELECT COUNT(*),
                    COALESCE(SUM(upstream_cost_usd), 0),
                    COUNT(*) FILTER (WHERE status_code >= 400),
                    MIN(created_at)::text,
                    MAX(created_at)::text
             FROM traces
             WHERE ($1::uuid IS NULL OR tenant_id = $1)
               AND created_at >= NOW() - ($2 * INTERVAL '1 day')",
        )
        .bind(tenant)
        .bind(days)
        .fetch_optional(&state.db)
        .await
        {
            Ok(row) => row,
            Err(e) => return db_err(e),
        };

    let (requests, total_cost, errors, first_at, last_at) =
        row.unwrap_or((0, Decimal::ZERO, 0, None, None));
    json_ok(json!({
        "scope": scope_label,
        "tenant_id": tenant.map(|t| t.to_string()),
        "window_days": days,
        "requests": requests,
        "errors": errors,
        "total_cost_usd": total_cost.to_string(),
        "first_trace_at": first_at,
        "last_trace_at": last_at,
    }))
}

async fn query_by_model(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
) -> Response {
    let rows: Vec<(String, i64, i64, i64, Decimal)> = match sqlx::query_as(
        "SELECT request_model, COUNT(*), COALESCE(SUM(prompt_tokens), 0),
                COALESCE(SUM(completion_tokens), 0), COALESCE(SUM(upstream_cost_usd), 0)
         FROM traces
         WHERE ($1::uuid IS NULL OR tenant_id = $1)
           AND created_at >= NOW() - ($2 * INTERVAL '1 day')
         GROUP BY request_model ORDER BY 5 DESC",
    )
    .bind(tenant)
    .bind(window.days())
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) => rows,
        Err(e) => return db_err(e),
    };

    let models: Vec<Value> = rows
        .into_iter()
        .map(|(model, requests, prompt_tokens, completion_tokens, cost)| {
            json!({
                "model": model,
                "requests": requests,
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "cost_usd": cost.to_string(),
            })
        })
        .collect();
    json_ok(json!({"models": models}))
}

async fn query_by_prompt(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
) -> Response {
    let rows: Vec<(Option<String>, i64, i64, Decimal)> = match sqlx::query_as(
        "SELECT system_prompt_hash, COUNT(*), COALESCE(SUM(prompt_tokens), 0),
                COALESCE(SUM(upstream_cost_usd), 0)
         FROM traces
         WHERE ($1::uuid IS NULL OR tenant_id = $1)
           AND created_at >= NOW() - ($2 * INTERVAL '1 day')
         GROUP BY system_prompt_hash ORDER BY 4 DESC LIMIT $3",
    )
    .bind(tenant)
    .bind(window.days())
    .bind(window.limit())
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) => rows,
        Err(e) => return db_err(e),
    };

    let prompts: Vec<Value> = rows
        .into_iter()
        .map(|(hash, requests, prompt_tokens, cost)| {
            json!({
                "system_prompt_hash": hash.unwrap_or_else(|| "(no system prompt)".to_string()),
                "requests": requests,
                "prompt_tokens": prompt_tokens,
                "cost_usd": cost.to_string(),
            })
        })
        .collect();
    json_ok(json!({"prompts": prompts}))
}

async fn query_waste(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
) -> Response {
    let rows: Vec<(String, i64, i64, i64, Decimal)> = match sqlx::query_as(
        "SELECT flag, COUNT(*), COALESCE(SUM(prompt_tokens), 0),
                COALESCE(SUM(completion_tokens), 0), COALESCE(SUM(upstream_cost_usd), 0)
         FROM traces
         CROSS JOIN LATERAL (VALUES
            ('over_provisioned', waste_detected->>'over_provisioned'),
            ('cache_miss', waste_detected->>'cache_miss_system_prompt'),
            ('model_misuse', waste_detected->>'model_misuse')) AS f(flag, fired)
         WHERE ($1::uuid IS NULL OR tenant_id = $1)
           AND created_at >= NOW() - ($2 * INTERVAL '1 day') AND f.fired = 'true'
         GROUP BY flag",
    )
    .bind(tenant)
    .bind(window.days())
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) => rows,
        Err(e) => return db_err(e),
    };

    let mut out = json!({});
    for (flag, requests, prompt_tokens, completion_tokens, cost) in rows {
        let savings = match flag.as_str() {
            "cache_miss" => Decimal::ZERO, // real saving needs provider caching
            _ => cost::savings_if_mini(prompt_tokens, completion_tokens),
        };
        out[flag] = json!({
            "requests": requests,
            "spend_usd": cost.to_string(),
            "savings_est_usd": savings.to_string(),
        });
    }
    json_ok(out)
}

/// GET /v1/dashboard — self-contained HTML shell; data loads via the JSON APIs.
pub async fn index() -> Html<&'static str> {
    Html(HTML)
}

/// GET /v1/dashboard/api/summary?days=7 (tenant scope)
pub async fn summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    match tenant_or(&state, &headers).await {
        Ok(id) => query_summary(&state, Some(id), &window, "tenant").await,
        Err(resp) => resp,
    }
}

/// GET /v1/dashboard/api/by-model?days=7 (tenant scope)
pub async fn by_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    match tenant_or(&state, &headers).await {
        Ok(id) => query_by_model(&state, Some(id), &window).await,
        Err(resp) => resp,
    }
}

/// GET /v1/dashboard/api/by-prompt?days=7&limit=10 (tenant scope)
pub async fn by_prompt(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    match tenant_or(&state, &headers).await {
        Ok(id) => query_by_prompt(&state, Some(id), &window).await,
        Err(resp) => resp,
    }
}

/// GET /v1/dashboard/api/waste?days=7 (tenant scope)
pub async fn waste(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    match tenant_or(&state, &headers).await {
        Ok(id) => query_waste(&state, Some(id), &window).await,
        Err(resp) => resp,
    }
}

/// GET /v1/dashboard/api/operator/tenants?days=7 — per-tenant spend + volume.
pub async fn operator_tenants(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    let rows: Vec<(String, i64, i64, Decimal)> = match sqlx::query_as(
        "SELECT tenant_id::text, COUNT(*), COUNT(*) FILTER (WHERE status_code >= 400),
                COALESCE(SUM(upstream_cost_usd), 0)
         FROM traces
         WHERE created_at >= NOW() - ($1 * INTERVAL '1 day')
         GROUP BY tenant_id ORDER BY 4 DESC",
    )
    .bind(window.days())
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) => rows,
        Err(e) => return db_err(e),
    };
    let tenants: Vec<Value> = rows
        .into_iter()
        .map(|(tenant_id, requests, errors, cost)| {
            json!({
                "tenant_id": tenant_id,
                "requests": requests,
                "errors": errors,
                "cost_usd": cost.to_string(),
            })
        })
        .collect();
    json_ok(json!({"tenants": tenants, "window_days": window.days()}))
}

/// GET /v1/dashboard/api/operator/summary?days=7 — platform-wide totals.
pub async fn operator_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    match operator_or(&state, &headers).await {
        Ok(()) => query_summary(&state, None, &window, "operator").await,
        Err(resp) => resp,
    }
}

/// GET /v1/dashboard/api/operator/by-model?days=7 — platform-wide spend by model.
pub async fn operator_by_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    match operator_or(&state, &headers).await {
        Ok(()) => query_by_model(&state, None, &window).await,
        Err(resp) => resp,
    }
}

/// GET /v1/dashboard/api/operator/by-prompt?days=7 — platform-wide clusters.
pub async fn operator_by_prompt(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    match operator_or(&state, &headers).await {
        Ok(()) => query_by_prompt(&state, None, &window).await,
        Err(resp) => resp,
    }
}

/// GET /v1/dashboard/api/operator/alerts?days=7 — cost anomaly episodes
/// (ITEM-020 AC2: an operator view lists them, firing and cleared).
pub async fn operator_alerts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    let alerts = match crate::alerts::list_alerts(&state.db, window.days()).await {
        Ok(rows) => rows,
        Err(e) => return internal(&e.to_string()),
    };
    let items: Vec<Value> = alerts
        .into_iter()
        .map(|a| {
            json!({
                "tenant_id": a.tenant_id.to_string(),
                "class": a.class,
                "status": a.status,
                "detail": a.detail,
                "threshold": a.threshold,
                "first_fired_at": a.first_fired_at,
                "last_fired_at": a.last_fired_at,
                "cleared_at": a.cleared_at,
            })
        })
        .collect();
    json_ok(json!({"alerts": items, "window_days": window.days()}))
}

/// GET /v1/dashboard/api/guard?days=7 (tenant scope, ITEM-026 AC2)
pub async fn guard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    match tenant_or(&state, &headers).await {
        Ok(id) => query_guard(&state, Some(id), &window, "tenant").await,
        Err(resp) => resp,
    }
}

/// One per-tenant row for the operator console (ITEM-037 AC3).
struct ConsoleTenant {
    tenant_id: String,
    requests: i64,
    errors: i64,
    guard_fallbacks: i64,
    cost: rust_decimal::Decimal,
}

async fn console_tenant_rows(
    pool: &sqlx::PgPool,
    days: i64,
) -> Result<Vec<ConsoleTenant>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, i64, i64, i64, rust_decimal::Decimal)>(
        "SELECT tenant_id::text,
                COUNT(*),
                COUNT(*) FILTER (WHERE status_code >= 400),
                COUNT(*) FILTER (WHERE guard_event->>'outcome' = 'fallback'),
                COALESCE(SUM(upstream_cost_usd), 0)
         FROM traces
         WHERE created_at >= NOW() - ($1 * INTERVAL '1 day')
         GROUP BY tenant_id ORDER BY 5 DESC",
    )
    .bind(days)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(tenant_id, requests, errors, guard_fallbacks, cost)| ConsoleTenant {
            tenant_id,
            requests,
            errors,
            guard_fallbacks,
            cost,
        })
        .collect())
}

async fn console_guard_outcomes(
    pool: &sqlx::PgPool,
    days: i64,
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT guard_event->>'outcome', COUNT(*)
         FROM traces
         WHERE created_at >= NOW() - ($1 * INTERVAL '1 day') AND guard_event IS NOT NULL
         GROUP BY 1 ORDER BY 2 DESC",
    )
    .bind(days)
    .fetch_all(pool)
    .await
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Render the server-side operator console page (ITEM-037 AC3): platform
/// totals, the per-tenant list (spend, requests, guard fallbacks), guard
/// outcome counts and the current alerts — read-only, no JS data fetches.
fn console_html(
    tenants: &[ConsoleTenant],
    guard_outcomes: &[(String, i64)],
    alerts: &[crate::alerts::AlertViewRow],
    days: i64,
) -> String {
    let totals_cost: rust_decimal::Decimal = tenants.iter().map(|t| t.cost).sum();
    let total_requests: i64 = tenants.iter().map(|t| t.requests).sum();
    let total_errors: i64 = tenants.iter().map(|t| t.errors).sum();
    let total_fallbacks: i64 = tenants.iter().map(|t| t.guard_fallbacks).sum();
    let tenant_rows = tenants
        .iter()
        .map(|t| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                html_escape(&t.tenant_id),
                t.requests,
                t.errors,
                t.guard_fallbacks,
                t.cost.to_string(),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let guard_rows = guard_outcomes
        .iter()
        .map(|(outcome, count)| {
            format!(
                "<tr><td>{}</td><td>{count}</td></tr>",
                html_escape(outcome)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let alert_rows = alerts
        .iter()
        .map(|a| {
            let detail = a.detail.to_string();
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                html_escape(&a.tenant_id.to_string()),
                html_escape(&a.class),
                html_escape(&a.status),
                html_escape(&detail),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Inferon — operator console</title>\
<style>body{{font:14px/1.5 system-ui,sans-serif;max-width:960px;margin:2rem auto;padding:0 1rem;color:#1a202c}}\
h1{{font-size:1.3rem}}table{{border-collapse:collapse;width:100%;margin:.5rem 0 1.5rem}}\
th,td{{border:1px solid #e2e8f0;padding:.25rem .6rem;text-align:left;font-size:.9rem}}\
.muted{{color:#718096}}</style></head>\
<body><h1>Inferon — operator console</h1>\
<p class=\"muted\">last {days} days · read-only</p>\
<p><b>{}</b> tenants · {total_requests} requests · {total_errors} errors · {total_fallbacks} guard fallbacks · <b>{totals_cost}</b> spend</p>\
<h2>Tenants</h2>\
<table><tr><th>tenant</th><th>requests</th><th>errors</th><th>guard fallbacks</th><th>spend</th></tr>\
{tenant_rows}</table>\
<h2>Schema guard outcomes</h2>\
<table><tr><th>outcome</th><th>count</th></tr>{guard_rows}</table>\
<h2>Alerts</h2>\
<table><tr><th>tenant</th><th>class</th><th>status</th><th>detail</th></tr>{alert_rows}</table>\
</body></html>",
        tenants.len(),
    )
}

/// GET /v1/dashboard/operator — the server-rendered operator console (ITEM-037
/// AC3). Gated exactly like the operator JSON APIs: an operator key or an
/// operator session; tenant credentials are 403.
pub async fn operator_console(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    let days = window.days();
    let tenants = match console_tenant_rows(&state.db, days).await {
        Ok(rows) => rows,
        Err(e) => return db_err(e),
    };
    let guard_outcomes = match console_guard_outcomes(&state.db, days).await {
        Ok(rows) => rows,
        Err(e) => return db_err(e),
    };
    let alerts = match crate::alerts::list_alerts(&state.db, days).await {
        Ok(rows) => rows,
        Err(e) => return internal(&e.to_string()),
    };
    let html = console_html(&tenants, &guard_outcomes, &alerts, days);
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

/// GET /v1/dashboard/api/operator/guard?days=7 — platform-wide (ITEM-026 AC2).
pub async fn operator_guard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    match operator_or(&state, &headers).await {
        Ok(()) => query_guard(&state, None, &window, "operator").await,
        Err(resp) => resp,
    }
}

#[derive(serde::Deserialize)]
pub struct KeyQuery {
    tenant_id: uuid::Uuid,
}

#[derive(serde::Deserialize)]
pub struct CreateKeyBody {
    tenant_id: uuid::Uuid,
    label: Option<String>,
}

/// GET /v1/dashboard/api/operator/keys?tenant_id=… — a tenant's key metadata
/// (ITEM-034 AC3). Key material is never returned — only lifecycle metadata.
pub async fn operator_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<KeyQuery>,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    match state.tenant_store.list_api_keys(query.tenant_id).await {
        Ok(keys) => {
            let rows: Vec<Value> = keys
                .into_iter()
                .map(|k| {
                    json!({
                        "id": k.id.to_string(),
                        "tenant_id": k.tenant_id.to_string(),
                        "label": k.label,
                        "created_at": k.created_at,
                        "revoked": k.revoked(),
                        "revoked_at": k.revoked_at,
                        "last_used_at": k.last_used_at,
                    })
                })
                .collect();
            json_ok(json!({"keys": rows}))
        }
        Err(e) => internal(&e.to_string()),
    }
}

/// POST /v1/dashboard/api/operator/keys — mint a tenant key (ITEM-034 AC3).
/// The raw key is returned exactly once; only its hash is stored.
pub async fn operator_keys_create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<CreateKeyBody>,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    let tenant = match state.tenant_store.tenant_by_id(body.tenant_id).await {
        Ok(Some(tenant)) => tenant,
        Ok(None) => {
            return crate::json_error(
                StatusCode::NOT_FOUND,
                "tenant_not_found",
                "no such tenant",
            )
        }
        Err(e) => return internal(&e.to_string()),
    };
    let label = body.label.unwrap_or_else(|| "default".to_string());
    match mint_key_payload(&state, tenant.id, &label).await {
        Ok(payload) => json_ok(payload),
        Err(e) => internal(&e),
    }
}

/// The single mint implementation (ITEM-034 AC3 path, shared with the tenant
/// dashboard — ITEM-038 AC3): a fresh raw key shown once, hash-only stored.
async fn mint_key_payload(
    state: &Arc<AppState>,
    tenant_id: uuid::Uuid,
    label: &str,
) -> Result<serde_json::Value, String> {
    let raw = crate::mint_client_key();
    let hash = crate::tenant_hash(&raw);
    let key_id = state
        .tenant_store
        .insert_api_key(tenant_id, label, &hash)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "key_id": key_id.to_string(),
        "tenant_id": tenant_id.to_string(),
        "label": label,
        "key": raw, // shown exactly once
    }))
}

#[derive(serde::Deserialize)]
pub struct KeyLabelBody {
    label: Option<String>,
}

/// GET /v1/dashboard/api/keys — the signed-in tenant's keys plus the caller's
/// role (ITEM-038 AC1). Never key material or hashes — lifecycle metadata only.
pub async fn tenant_keys(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    match state.tenant_store.list_api_keys(tenant_id).await {
        Ok(keys) => {
            let rows: Vec<Value> = keys
                .into_iter()
                .map(|k| {
                    json!({
                        "id": k.id.to_string(),
                        "label": k.label,
                        "created_at": k.created_at,
                        "revoked": k.revoked(),
                        "revoked_at": k.revoked_at,
                        "last_used_at": k.last_used_at,
                    })
                })
                .collect();
            json_ok(json!({ "role": role, "keys": rows }))
        }
        Err(e) => internal(&e.to_string()),
    }
}

/// POST /v1/dashboard/api/keys — create a key for the signed-in tenant
/// (ITEM-038 AC1; owner and members may create, AC2). Raw key shown once.
pub async fn tenant_key_create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<KeyLabelBody>,
) -> Response {
    let (tenant_id, _role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    let label = body.label.unwrap_or_else(|| "default".to_string());
    match mint_key_payload(&state, tenant_id, &label).await {
        Ok(payload) => json_ok(payload),
        Err(e) => internal(&e),
    }
}

/// POST /v1/dashboard/api/keys/{key_id}/revoke — owners only (ITEM-038 AC2:
/// members cannot revoke). Scoped to the caller's tenant — another tenant's
/// key id is not found here.
pub async fn tenant_key_revoke(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_id): Path<uuid::Uuid>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if role != "owner" {
        return forbidden();
    }
    match state.tenant_store.revoke_tenant_key(tenant_id, key_id).await {
        Ok(true) => json_ok(json!({ "key_id": key_id.to_string(), "revoked": true })),
        Ok(false) => crate::json_error(
            StatusCode::NOT_FOUND,
            "key_not_found",
            "no such key for this tenant",
        ),
        Err(e) => internal(&e.to_string()),
    }
}

/// POST /v1/dashboard/api/operator/keys/{key_id}/revoke — revoke a key
/// (ITEM-034 AC2: the next request with it is 401).
pub async fn operator_key_revoke(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_id): Path<uuid::Uuid>,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    match state.tenant_store.revoke_api_key(key_id).await {
        Ok(true) => json_ok(json!({"key_id": key_id.to_string(), "revoked": true})),
        Ok(false) => crate::json_error(
            StatusCode::NOT_FOUND,
            "key_not_found",
            "no such active key",
        ),
        Err(e) => internal(&e.to_string()),
    }
}

/// Guard telemetry aggregate over the additive `guard_event` JSONB on traces
/// (ITEM-026 AC2/AC3). `tenant` is Some(id) for tenant scope, None for the
/// operator platform-wide view — same shape trick as the other queries.
/// Every event carries an outcome (flushed | fallback | fallback_failed) and,
/// on failures, the GuardReason label; counts break down per guarded model and
/// per failure reason, and fallbacking prompts (negative samples) come back in
/// the existing system-prompt-hash shape for later distillation harvests.
async fn query_guard(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
    scope_label: &str,
) -> Response {
    let days = window.days();
    let row: Option<(i64, i64, i64)> = match sqlx::query_as(
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
    .await
    {
        Ok(row) => row,
        Err(e) => return db_err(e),
    };
    let (guarded, fallback, fallback_failed) = row.unwrap_or((0, 0, 0));

    let by_model: Vec<(String, i64, i64, i64)> = match sqlx::query_as(
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
    .await
    {
        Ok(rows) => rows,
        Err(e) => return db_err(e),
    };
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

    let by_reason: Vec<(String, i64)> = match sqlx::query_as(
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
    .await
    {
        Ok(rows) => rows,
        Err(e) => return db_err(e),
    };
    let reasons: Vec<Value> = by_reason
        .into_iter()
        .map(|(reason, events)| json!({ "reason": reason, "events": events }))
        .collect();

    let negative: Vec<(String, i64)> = match sqlx::query_as(
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
    .await
    {
        Ok(rows) => rows,
        Err(e) => return db_err(e),
    };
    let negative_samples: Vec<Value> = negative
        .into_iter()
        .map(|(hash, fallbacks)| json!({ "system_prompt_hash": hash, "fallbacks": fallbacks }))
        .collect();

    json_ok(json!({
        "scope": scope_label,
        "tenant_id": tenant.map(|t| t.to_string()),
        "window_days": days,
        "guarded_requests": guarded,
        "fallback_requests": fallback,
        "fallback_failed": fallback_failed,
        "by_model": models,
        "by_reason": reasons,
        "negative_samples": negative_samples,
    }))
}

/// Self-contained dashboard page: key input (kept in memory), window buttons,
/// and CSS-bar charts fed by the JSON APIs above.
const HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8"><title>Inferon cost dashboard</title>
<style>
  body { font: 14px/1.5 system-ui, sans-serif; max-width: 900px; margin: 2rem auto; padding: 0 1rem; color: #1a202c; }
  h1 { font-size: 1.3rem; } h2 { font-size: 1.05rem; margin-top: 1.5rem; }
  .keyline input { width: 26rem; font-family: monospace; }
  button { margin-right: .25rem; padding: .15rem .6rem; cursor: pointer; }
  .cards { display: flex; gap: 1rem; flex-wrap: wrap; margin: 1rem 0; }
  .card { border: 1px solid #e2e8f0; border-radius: 8px; padding: .5rem 1rem; min-width: 9rem; }
  .card .num { font-size: 1.4rem; font-weight: 600; }
  .bar { background: #3182ce; color: #fff; padding: 2px 8px; margin: 2px 0; border-radius: 3px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 100%; }
  .wasteflag { margin: .35rem 0; }
  .err { color: #c53030; }
  .empty { border: 1px dashed #cbd5e0; border-radius: 8px; padding: 1.5rem; margin: 1rem 0; color: #4a5568; background: #f7fafc; }
  .empty code { background: #edf2f7; padding: .1rem .35rem; border-radius: 3px; }
  .why { color: #4a5568; font-size: .9rem; margin-top: .1rem; }
  .muted { color: #718096; }
</style>
</head>
<body>
<h1>Inferon — cost leaks</h1>
<p class="muted" id="session"></p>
<p class="keyline">API key <input id="key" type="password" placeholder="sk-..." autocomplete="off">
  <button onclick="setDays(7)">7d</button><button onclick="setDays(30)">30d</button><button onclick="setDays(90)">90d</button>
  <span id="days"></span></p>
<div class="cards" id="cards"></div>
<h2>Spend by model</h2><div id="byModel"></div>
<h2>Top system prompts by spend</h2><div id="byPrompt"></div>
<h2>Waste flags &amp; estimated $ impact</h2><div id="waste"></div>
<h2>API keys</h2><div id="keysReveal"></div><div id="keys"></div>
<p class="err" id="err"></p>
<script>
let days = 7;
let session = null; // set by renderSession(): the signed-in user or null
const KEY = () => document.getElementById('key').value.trim();
const $ = id => document.getElementById(id);
function esc(s) { return String(s).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c])); }
// Render a USD amount from its decimal string without ever going through a
// float (floats print 1.6e-5, strings print $0.000016 — ITEM-019 AC3).
function usd(s) {
  let v = String(s);
  const neg = v.startsWith('-');
  if (neg) v = v.slice(1);
  let [whole, frac] = v.split('.');
  if (frac !== undefined) {
    frac = frac.replace(/0+$/, '');
    v = frac ? whole + '.' + frac : whole;
  }
  if (v === '' || v === '0' || /^0\.0*$/.test(v)) v = '0.00';
  // Thousands separators on the integer part only.
  v = v.replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  return (neg ? '-' : '') + '$' + v;
}
function emptyBox(text) {
  return '<div class="empty">' + text + '</div>';
}
function card(label, num) { return '<div class="card"><div>' + esc(label) + '</div><div class="num">' + esc(num) + '</div></div>'; }
function bars(items, key) {
  const max = Math.max(...items.map(i => parseFloat(i[key])), 0);
  return items.map(i => {
    const w = max > 0 ? Math.max(3, Math.round(100 * parseFloat(i[key]) / max)) : 0;
    return '<div class="bar" style="width:' + w + '%">' + esc(i.label) + ' — ' + usd(i[key]) + '</div>';
  }).join('') || '<p class="muted">no data in this window</p>';
}
async function get(path) {
  // A signed-in session needs no API key (ITEM-036 AC2); the key is only
  // attached when one was entered.
  const h = {};
  const k = KEY();
  if (k) h.Authorization = 'Bearer ' + k;
  const r = await fetch(path, { headers: h });
  if (!r.ok) throw new Error((await r.json()).error?.message || r.status);
  return r.json();
}
async function load() {
  $('err').textContent = '';
  if (!KEY() && !session) { $('err').textContent = 'enter your API key or sign in'; return; }
  try {
    const q = '?days=' + days;
    const s = await get('/v1/dashboard/api/summary' + q);
    $('days').textContent = 'last ' + s.window_days + ' days';
    if (s.requests === 0) {
      // AC2: a meaningful empty state, not bare zeros.
      $('cards').innerHTML = emptyBox('No requests yet — point your baseURL at us and make one, e.g. ' +
        '<code>client = OpenAI(base_url=&quot;' + location.origin + '/v1&quot;, api_key=...)</code>');
      $('byModel').innerHTML = ''; $('byPrompt').innerHTML = ''; $('waste').innerHTML = '';
      return;
    }
    $('cards').innerHTML =
      card('Total spend', usd(s.total_cost_usd)) +
      card('Requests', s.requests) +
      card('Errors (4xx/5xx)', s.errors) +
      card('Tenant', String(s.tenant_id).slice(0, 8) + '…');
    const m = await get('/v1/dashboard/api/by-model' + q);
    $('byModel').innerHTML = m.models.length
      ? bars(m.models.map(x => ({ label: x.model, cost_usd: x.cost_usd })), 'cost_usd')
      : emptyBox('No requests in this window.');
    const p = await get('/v1/dashboard/api/by-prompt' + q);
    $('byPrompt').innerHTML = p.prompts.length
      ? bars(p.prompts.map(x => ({ label: x.system_prompt_hash.slice(0, 12), cost_usd: x.cost_usd })), 'cost_usd')
      : emptyBox('No system prompts in this window.');
    const w = await get('/v1/dashboard/api/waste' + q);
    // AC5: each rule explains *why* it is a leak, in one plain-English line.
    const rules = {
      over_provisioned: {
        name: 'Over-provisioned (gpt-4o, tiny output)',
        why: 'A gpt-4o-tier model answered a tiny request (<100 prompt / <50 completion tokens). gpt-4o-mini costs ~6% as much for replies like this.'
      },
      cache_miss: {
        name: 'Cache-miss system prompts',
        why: 'A 4,000+ token system prompt repeated with no provider cache benefit — every call re-bills the whole prefix. Ask DeepSeek to cache it or shorten the prompt.'
      },
      model_misuse: {
        name: 'gpt-4o on simple text',
        why: 'gpt-4o handled plain short text with no tools or formatting. gpt-4o-mini fits this shape and costs a fraction of the price.'
      }
    };
    $('waste').innerHTML = Object.keys(rules).map(k => {
      const v = w[k];
      if (!v) return '';
      const saving = parseFloat(v.savings_est_usd) > 0 ? ' — ~' + usd(v.savings_est_usd) + ' recoverable on gpt-4o-mini' : '';
      return '<div class="wasteflag"><b>' + esc(rules[k].name) + '</b>: ' + v.requests + ' requests, ' + usd(v.spend_usd) +
             ' spent' + saving + '<div class="why">Why: ' + rules[k].why + '</div></div>';
    }).join('') || emptyBox('No waste flagged — clean traffic. <span class="muted">(Waste rules only apply to gpt-4o-tier usage.)</span>');
  } catch (e) { $('err').textContent = 'error: ' + e.message; }
}
function setDays(d) { days = d; load(); }
document.getElementById('key').addEventListener('keydown', e => { if (e.key === 'Enter') load(); });

// Account strip (ITEM-035 AC4): /auth/me answers 200 when a session cookie
// exists and 401 otherwise — the shell renders sign-in or log-out state
// client-side. The API key stays out of this call (no Authorization header).
async function renderSession() {
  const el = $('session');
  let signedIn = null;
  try {
    const r = await fetch('/auth/me', { headers: { Accept: 'application/json' } });
    if (r.ok) signedIn = await r.json();
  } catch (e) { /* signed out or unreachable — show the sign-in link */ }
  session = signedIn;
  if (signedIn) loadKeys(); // the keys panel (ITEM-038) needs the session
  if (signedIn) {
    el.innerHTML = 'Signed in as <b>' + esc(signedIn.github_login) + '</b> · ' +
      '<a id="signOutLink" href="/auth/logout">log out</a>';
  } else {
    el.innerHTML = '<a id="signOutLink" href="/auth/login">Sign in with GitHub</a>';
  }
  const link = $('signOutLink');
  if (link && signedIn) link.onclick = e => { e.preventDefault(); signOut(); };
}
async function signOut() {
  try { await fetch('/auth/logout', { method: 'POST' }); } catch (e) { /* best effort */ }
  location.reload();
}

// Key management (ITEM-038 AC1/AC2): signed-in owners/members manage the
// tenant's keys through the session — a created key is revealed exactly once.
// The endpoints enforce roles (revoke is owner-only); the UI mirrors it.
async function loadKeys() {
  const el = $('keys');
  if (!session) { el.innerHTML = ''; return; }
  let data;
  try {
    const r = await fetch('/v1/dashboard/api/keys');
    if (!r.ok) throw new Error(r.status);
    data = await r.json();
  } catch (e) {
    el.innerHTML = emptyBox('Could not load keys — are you signed in?');
    return;
  }
  const rows = data.keys.map(k => {
    const revoke = (data.role === 'owner' && !k.revoked)
      ? ' <button onclick="revokeKey(\'' + k.id + '\')">revoke</button>' : '';
    return '<div class="keyline">' + esc(k.label) + ' — ' + (k.revoked ? 'revoked' : 'active') +
      ' · created ' + esc(k.created_at) + revoke + '</div>';
  }).join('') || '<p class="muted">no keys yet</p>';
  const form = (data.role === 'owner')
    ? '<p class="keyline">label <input id="newKeyLabel" placeholder="ci"> ' +
      '<button onclick="createKey()">new key</button></p>'
    : '<p class="muted">member — you can create keys; only the owner can revoke.</p>';
  el.innerHTML = form + rows;
}
async function createKey() {
  const input = $('newKeyLabel');
  const label = (input && input.value.trim()) || 'default';
  let data;
  try {
    const r = await fetch('/v1/dashboard/api/keys', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ label: label })
    });
    if (!r.ok) throw new Error(r.status);
    data = await r.json();
  } catch (e) { $('err').textContent = 'create failed: ' + e.message; return; }
  $('keysReveal').innerHTML = 'Your new key: <code>' + esc(data.key) + '</code> — stored hashed, <b>not shown again</b>. Copy it now.';
  loadKeys();
}
async function revokeKey(id) {
  let ok;
  try {
    const r = await fetch('/v1/dashboard/api/keys/' + id + '/revoke', { method: 'POST' });
    ok = r.ok;
  } catch (e) { ok = false; }
  if (!ok) { $('err').textContent = 'revoke failed (owners only)'; return; }
  loadKeys();
}
renderSession();
</script>
</body>
</html>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TenantStore;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    /// Router state with an InMemory tenant store; `db` is a lazy pool that is
    /// never actually queried by the auth/index tests below.
    const SEEDED_TENANT: &str = "22222222-2222-2222-2222-222222222222";

    fn seeded_tenant_store() -> std::sync::Arc<crate::InMemoryTenantStore> {
        let mut tenants = std::collections::HashMap::new();
        let hash = {
            use sha2::{Digest, Sha256};
            hex::encode(Sha256::digest(b"sk-dashboard-dev"))
        };
        tenants.insert(
            hash,
            crate::Tenant {
                id: uuid::Uuid::parse_str(SEEDED_TENANT).unwrap(),
                upstream_openai_key: Some("sk-master".to_string()),
            },
        );
        std::sync::Arc::new(crate::InMemoryTenantStore::new(tenants))
    }

    fn state_with(
        accounts: Option<std::sync::Arc<crate::auth::Accounts>>,
        tenant_store: std::sync::Arc<dyn crate::TenantStore>,
    ) -> std::sync::Arc<crate::AppState> {
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        std::sync::Arc::new(crate::AppState {
            http_client: reqwest::Client::new(),
            upstream_url: "http://127.0.0.1:1/".to_string(),
            tenant_store,
            budget_store: std::sync::Arc::new(crate::budget::InMemoryBudgetStore::default()),
            telemetry_tx: tx,
            operator_key: Some("op-secret".to_string()),
            accounts,
            db: sqlx::postgres::PgPoolOptions::new()
                .acquire_timeout(std::time::Duration::from_millis(300))
                .connect_lazy("postgres://inferon:inferon@127.0.0.1:1/inferon")
                .unwrap(),
        })
    }

    fn dashboard_state() -> std::sync::Arc<crate::AppState> {
        dashboard_state_full(None)
    }

    /// Same seeded state, with the given accounts (ITEM-036 session-scope
    /// tests inject an OAuth-configured `accounts` here).
    fn dashboard_state_full(
        accounts: Option<std::sync::Arc<crate::auth::Accounts>>,
    ) -> std::sync::Arc<crate::AppState> {
        state_with(accounts, seeded_tenant_store())
    }

    fn get(uri: &str, key: Option<&str>) -> axum::http::Request<Body> {
        let mut builder = Request::builder().method("GET").uri(uri);
        if let Some(key) = key {
            builder = builder.header("authorization", format!("Bearer {key}"));
        }
        builder.body(Body::empty()).unwrap()
    }

    fn post(uri: &str, key: Option<&str>, body: &str) -> axum::http::Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(key) = key {
            builder = builder.header("authorization", format!("Bearer {key}"));
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    #[tokio::test]
    async fn operator_key_endpoints_gate_then_manage_keys() {
        // ITEM-034 AC2/AC3: the operator key API is gated like the other
        // operator views and drives the mint/list/revoke lifecycle over the
        // in-memory store (same semantics as the Postgres store).
        let state = dashboard_state();
        let router = crate::build_router(state);
        const TID: &str = "22222222-2222-2222-2222-222222222222";
        let keys_uri = "/v1/dashboard/api/operator/keys";
        let list_uri = format!("{keys_uri}?tenant_id={TID}");

        // Scope gates (no key -> 401, tenant key -> 403).
        let missing_list = router.clone().oneshot(get(&list_uri, None)).await.unwrap();
        assert_eq!(missing_list.status(), axum::http::StatusCode::UNAUTHORIZED, "list no key");
        let tenant_list = router
            .clone()
            .oneshot(get(&list_uri, Some("sk-dashboard-dev")))
            .await
            .unwrap();
        assert_eq!(tenant_list.status(), axum::http::StatusCode::FORBIDDEN, "list tenant key");
        let tenant_create = router
            .clone()
            .oneshot(post(
                keys_uri,
                Some("sk-dashboard-dev"),
                &format!(r#"{{"tenant_id":"{TID}"}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(tenant_create.status(), axum::http::StatusCode::FORBIDDEN, "create tenant key");

        // Mint: operator key -> raw key shown once, key id returned.
        let create = router
            .clone()
            .oneshot(post(
                keys_uri,
                Some("op-secret"),
                &format!(r#"{{"tenant_id":"{TID}","label":"ci"}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(create.status(), axum::http::StatusCode::OK);
        let created: Value = serde_json::from_slice(
            &axum::body::to_bytes(create.into_body(), 1024 * 1024).await.unwrap(),
        )
        .unwrap();
        let raw_key = created["key"].as_str().expect("raw key shown once").to_string();
        let key_id = created["key_id"].as_str().expect("key id").to_string();
        assert!(raw_key.starts_with(crate::CLIENT_KEY_PREFIX));

        // List shows metadata — never key material.
        let listed = router
            .clone()
            .oneshot(get(&list_uri, Some("op-secret")))
            .await
            .unwrap();
        assert_eq!(listed.status(), axum::http::StatusCode::OK);
        let list_json: Value = serde_json::from_slice(
            &axum::body::to_bytes(listed.into_body(), 1024 * 1024).await.unwrap(),
        )
        .unwrap();
        let list_text = list_json.to_string();
        assert!(list_text.contains("ci"), "label present");
        assert!(
            !list_text.contains(&raw_key) && !list_text.contains("sk-inferon-"),
            "list never returns key material"
        );

        // Revoke -> listed as revoked; the raw key stops authenticating is
        // pinned by the proxy seam test (lib.rs).
        let revoke = router
            .clone()
            .oneshot(post(
                &format!("/v1/dashboard/api/operator/keys/{key_id}/revoke"),
                Some("op-secret"),
                "",
            ))
            .await
            .unwrap();
        let revoke_status = revoke.status();
        let revoke_body = axum::body::to_bytes(revoke.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(
            revoke_status,
            axum::http::StatusCode::OK,
            "revoke body: {} (key_id {key_id})",
            String::from_utf8_lossy(&revoke_body)
        );
        let listed = router
            .clone()
            .oneshot(get(&list_uri, Some("op-secret")))
            .await
            .unwrap();
        let list_json: Value = serde_json::from_slice(
            &axum::body::to_bytes(listed.into_body(), 1024 * 1024).await.unwrap(),
        )
        .unwrap();
        let ci = list_json["keys"]
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["label"] == "ci")
            .expect("ci key listed");
        assert_eq!(ci["revoked"], true);

        // Unknown tenant mint -> 404; unknown key revoke -> 404.
        let unknown_tenant = router
            .clone()
            .oneshot(post(
                keys_uri,
                Some("op-secret"),
                r#"{"tenant_id":"99999999-9999-9999-9999-999999999999","label":"x"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(unknown_tenant.status(), axum::http::StatusCode::NOT_FOUND);
        let unknown_key = router
            .clone()
            .oneshot(post(
                "/v1/dashboard/api/operator/keys/99999999-9999-9999-9999-999999999999/revoke",
                Some("op-secret"),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(unknown_key.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn index_serves_the_dashboard_shell() {
        let response = crate::build_router(dashboard_state())
            .oneshot(get("/v1/dashboard", None))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains("Inferon — cost leaks"));
        assert!(html.contains("/v1/dashboard/api/summary"));
    }

    #[tokio::test]
    async fn dashboard_apis_require_a_valid_tenant_key() {
        let state = dashboard_state();
        let router = crate::build_router(state);
        for uri in [
            "/v1/dashboard/api/summary",
            "/v1/dashboard/api/by-model",
            "/v1/dashboard/api/by-prompt",
            "/v1/dashboard/api/waste",
        ] {
            let missing = router
                .clone()
                .oneshot(get(uri, None))
                .await
                .unwrap();
            assert_eq!(missing.status(), axum::http::StatusCode::UNAUTHORIZED, "{uri} without key");
            let unknown = router
                .clone()
                .oneshot(get(uri, Some("sk-wrong")))
                .await
                .unwrap();
            assert_eq!(unknown.status(), axum::http::StatusCode::UNAUTHORIZED, "{uri} with unknown key");
        }
    }

    #[tokio::test]
    async fn operator_scope_is_enforced_before_any_query() {
        let state = dashboard_state();
        let router = crate::build_router(state.clone());
        let uri = "/v1/dashboard/api/operator/tenants?days=7";

        // Endpoint gates (no DB touched): missing -> 401, tenant key -> 403.
        let missing = router
            .clone()
            .oneshot(get(uri, None))
            .await
            .unwrap();
        assert_eq!(missing.status(), axum::http::StatusCode::UNAUTHORIZED);
        let tenant_key = router
            .clone()
            .oneshot(get(uri, Some("sk-dashboard-dev")))
            .await
            .unwrap();
        assert_eq!(tenant_key.status(), axum::http::StatusCode::FORBIDDEN);

        // Tenant endpoints reject the operator credential.
        let op_on_tenant_view = router
            .clone()
            .oneshot(get("/v1/dashboard/api/summary", Some("op-secret")))
            .await
            .unwrap();
        assert_eq!(op_on_tenant_view.status(), axum::http::StatusCode::FORBIDDEN);

        // ITEM-026: the guard views share the same scope gates (no DB touched
        // before the gate): tenant guard view needs a tenant key, operator
        // guard view needs the operator key.
        let tenant_guard = "/v1/dashboard/api/guard?days=7";
        let guard_missing = router.clone().oneshot(get(tenant_guard, None)).await.unwrap();
        assert_eq!(guard_missing.status(), axum::http::StatusCode::UNAUTHORIZED);
        let guard_op = router.clone().oneshot(get(tenant_guard, Some("op-secret"))).await.unwrap();
        assert_eq!(guard_op.status(), axum::http::StatusCode::FORBIDDEN);

        let operator_guard = "/v1/dashboard/api/operator/guard?days=7";
        let op_guard_missing = router.clone().oneshot(get(operator_guard, None)).await.unwrap();
        assert_eq!(op_guard_missing.status(), axum::http::StatusCode::UNAUTHORIZED);
        let op_guard_tenant = router
            .clone()
            .oneshot(get(operator_guard, Some("sk-dashboard-dev")))
            .await
            .unwrap();
        assert_eq!(op_guard_tenant.status(), axum::http::StatusCode::FORBIDDEN);

        // ITEM-020 AC2: the operator alerts view has the same gate.
        let alerts_uri = "/v1/dashboard/api/operator/alerts?days=7";
        let alerts_no_key = router.clone().oneshot(get(alerts_uri, None)).await.unwrap();
        assert_eq!(alerts_no_key.status(), axum::http::StatusCode::UNAUTHORIZED);
        let alerts_tenant = router
            .clone()
            .oneshot(get(alerts_uri, Some("sk-dashboard-dev")))
            .await
            .unwrap();
        assert_eq!(alerts_tenant.status(), axum::http::StatusCode::FORBIDDEN);

        // Scope resolution itself (pure, no DB): operator key -> Operator,
        // tenant key -> Tenant, unknown -> None.
        fn bearer(key: &str) -> axum::http::HeaderMap {
            let mut h = axum::http::HeaderMap::new();
            h.insert("authorization", format!("Bearer {key}").parse().unwrap());
            h
        }
        assert!(matches!(
            resolve_scope(&state, &bearer("op-secret")).await.unwrap(),
            Scope::Operator
        ));
        assert!(matches!(
            resolve_scope(&state, &bearer("sk-dashboard-dev")).await.unwrap(),
            Scope::Tenant(_)
        ));
        assert!(resolve_scope(&state, &bearer("sk-nope")).await.is_err());

        // Operator access is disabled when no operator key is configured.
        let mut disabled = dashboard_state();
        std::sync::Arc::get_mut(&mut disabled)
            .unwrap()
            .operator_key = None;
        assert!(resolve_scope(&disabled, &bearer("op-secret")).await.is_err());
    }

    #[test]
    fn window_defaults_and_clamps() {
        assert_eq!(Window { days: None, limit: None }.days(), 7);
        assert_eq!(Window { days: Some(3), limit: None }.days(), 3);
        assert_eq!(Window { days: Some(9999), limit: None }.days(), 365);
        assert_eq!(Window { days: Some(0), limit: None }.days(), 1);
        assert_eq!(Window { days: None, limit: Some(500) }.limit(), 100);
        assert_eq!(Window { days: None, limit: Some(3) }.limit(), 3);
    }

    // --- ITEM-019 dashboard polish ---

    /// AC1: the served HTML is fully self-contained and origin-relative — no
    /// absolute http(s) URLs, no localhost/internal hostnames in markup or JS.
    #[test]
    fn html_has_no_absolute_or_internal_references() {
        for forbidden in ["http://", "https://", "localhost", "127.0.0.1", "0.0.0.0"] {
            assert!(
                !HTML.contains(forbidden),
                "served HTML must not contain {forbidden:?} (mixed-content / internal ref)"
            );
        }
    }

    /// AC1: every data fetch uses a same-origin relative path.
    #[test]
    fn html_fetches_are_origin_relative() {
        for api in [
            "/v1/dashboard/api/summary",
            "/v1/dashboard/api/by-model",
            "/v1/dashboard/api/by-prompt",
            "/v1/dashboard/api/waste",
        ] {
            assert!(
                HTML.contains(&format!("get('{api}'")),
                "fetch for {api} must be a relative path"
            );
        }
    }

    /// AC2: a zero-request tenant sees a meaningful empty state, not bare zeros.
    #[test]
    fn html_empty_state_guides_a_new_developer() {
        let lower = HTML.to_ascii_lowercase();
        assert!(
            lower.contains("no requests yet") && lower.contains("point your baseurl"),
            "empty state must say 'no requests yet — point your baseURL at us and make one'"
        );
        // The message must render when the summary reports zero requests.
        assert!(HTML.contains("s.requests === 0"), "empty state wired to the summary count");
    }

    /// ITEM-035 AC4: the shell shows sign-in/log-out state, driven by /auth/me
    /// without ever attaching the tenant API key (sessions are cookie-based).
    #[test]
    fn html_has_a_sign_in_state_affordance() {
        assert!(HTML.contains("id=\"session\""), "session strip element");
        assert!(HTML.contains("href=\"/auth/login\""), "sign-in link");
        assert!(HTML.contains("Sign in with GitHub"), "sign-in text");
        assert!(HTML.contains("fetch('/auth/me'"), "me fetched bearer-free");
        assert!(
            HTML.contains("fetch('/auth/logout', { method: 'POST' })"),
            "logout is a POST"
        );
        assert!(HTML.contains("signedIn.github_login"), "renders the github login");
        // The account calls must not ride the keyed get() helper.
        assert!(
            !HTML.contains("get('/auth/me'") && !HTML.contains("get('/auth/logout'"),
            "me/logout are not keyed API calls"
        );
        // ITEM-036 AC2: a signed-in session lets the shell load without a key.
        assert!(HTML.contains("if (k) h.Authorization"), "key attached only when entered");
        assert!(
            HTML.contains("!KEY() && !session"),
            "the shell loads for a signed-in session with no key"
        );
    }

    /// ITEM-038 AC1/AC2: the signed-in shell manages the tenant's keys — list,
    /// create (raw key shown once) and owner-only revoke.
    #[test]
    fn html_has_a_keys_management_panel_for_signed_in_users() {
        assert!(HTML.contains("<h2>API keys</h2>"), "keys section");
        assert!(HTML.contains("id=\"keys\""), "keys list container");
        assert!(
            HTML.contains("fetch('/v1/dashboard/api/keys')"),
            "keys load through the session, no bearer"
        );
        assert!(HTML.contains("method: 'POST'") && HTML.contains("/v1/dashboard/api/keys"));
        assert!(
            HTML.contains("/v1/dashboard/api/keys/' + id + '/revoke"),
            "revoke hits the scoped endpoint"
        );
        assert!(
            HTML.contains("not shown again"),
            "a created key is revealed exactly once"
        );
        assert!(
            HTML.contains("data.role === 'owner'") && HTML.contains("only the owner can revoke"),
            "role mirrors the API permission split"
        );
        assert!(HTML.contains("if (signedIn) loadKeys()"), "panel loads with the session");
    }

    /// AC3: currency is rendered from the decimal string, never through a
    /// float that could print scientific notation like 1.6e-5.
    #[test]
    fn html_renders_currency_without_float_notation() {
        assert!(
            HTML.contains("function usd("),
            "a string-based usd() formatter must exist"
        );
        assert!(
            !HTML.contains("toExponential"),
            "scientific notation is never used for display"
        );
        // Bar widths may scale from numbers; displayed $ amounts must not.
        let scripts = HTML[HTML.find("<script>").unwrap()..].to_string();
        let display_lines: Vec<&str> = scripts
            .lines()
            .filter(|l| l.contains("$") && l.contains("+"))
            .collect();
        for line in display_lines {
            assert!(
                !line.contains("parseFloat"),
                "displayed currency must not pass through parseFloat: {line}"
            );
        }
    }

    /// AC4: the window selector offers 7/30/90 and the default is 7.
    #[test]
    fn html_window_selector_offers_7_30_90() {
        assert!(HTML.contains("let days = 7"), "default window is 7 days");
        for d in [7, 30, 90] {
            assert!(
                HTML.contains(&format!("setDays({d})")),
                "window selector must offer {d} days"
            );
        }
        assert!(
            !HTML.contains("setDays(1)"),
            "the old 1-day preset is gone (7/30/90 per ITEM-019 AC4)"
        );
    }

    /// AC5: the waste view explains *why* each rule fires, in plain English.
    #[test]
    fn html_waste_view_explains_each_rule() {
        for (flag, snippet) in [
            ("over_provisioned", "tiny"),
            ("cache_miss", "cache"),
            ("model_misuse", "simple text"),
        ] {
            assert!(
                HTML.contains(&format!("{flag}: {{")),
                "waste rules must carry a why-explanation entry for {flag}"
            );
            assert!(
                HTML.contains(snippet),
                "{flag} explanation must mention: {snippet}"
            );
        }
    }

    // --- ITEM-036 AC2: a signed-in owner/member reaches tenant scope via the
    // session cookie (dashboard resolve_scope session path) ---

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn cookie_headers(token: &str) -> axum::http::HeaderMap {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            format!("inferon_session={token}").parse().unwrap(),
        );
        headers
    }

    /// State where user 42 owns tenant 2222… (the seeded tenant) and holds a
    /// valid session token; returns (state, token, tenant_id).
    async fn signed_in_state() -> (std::sync::Arc<crate::AppState>, String, uuid::Uuid) {
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let user = users.upsert_github_user(42, "octocat", false).await.expect("ok");
        let tenant_id = uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        users.add_membership(tenant_id, user.id, "owner").await.expect("ok");
        let state = dashboard_state_full(Some(crate::auth::test_helpers::dummy_accounts(users)));
        let token = crate::session::issue_session(
            user.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        (state, token, tenant_id)
    }

    #[tokio::test]
    async fn session_cookie_grants_the_owners_tenant_scope_without_a_key() {
        let (state, token, tenant_id) = signed_in_state().await;
        let scope = resolve_scope(&state, &cookie_headers(&token)).await.expect("resolved");
        assert!(matches!(scope, Scope::Tenant(id) if id == tenant_id));
    }

    #[tokio::test]
    async fn missing_or_forged_session_stays_unauthorized() {
        // No accounts configured: the session path is inert (pre-036 behavior).
        let state = dashboard_state();
        let err = resolve_scope(&state, &axum::http::HeaderMap::new()).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::UNAUTHORIZED);

        // Forged cookie (wrong secret) with accounts configured.
        let (state, _, _) = signed_in_state().await;
        let forged = crate::session::issue_session(
            uuid::Uuid::new_v4(),
            "not-the-secret",
            now_secs(),
            3600,
        );
        let err = resolve_scope(&state, &cookie_headers(&forged)).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_memberless_signed_in_user_stays_unauthorized() {
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let user = users.upsert_github_user(7, "nobody", false).await.expect("ok");
        let state = dashboard_state_full(Some(crate::auth::test_helpers::dummy_accounts(users)));
        let token = crate::session::issue_session(
            user.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        let err = resolve_scope(&state, &cookie_headers(&token)).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn bearer_keys_and_operator_credentials_win_over_the_session() {
        // The user's session points at tenant A, but the bearer key belongs to
        // the seeded tenant 2222… and the operator key is op-secret — the
        // bearer paths must resolve first (SDKs stay key-authed, additive).
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let user = users.upsert_github_user(9, "owner", false).await.expect("ok");
        users
            .add_membership(
                uuid::Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap(),
                user.id,
                "owner",
            )
            .await
            .expect("ok");
        let state = dashboard_state_full(Some(crate::auth::test_helpers::dummy_accounts(users)));
        let token = crate::session::issue_session(
            user.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        let mut headers = cookie_headers(&token);
        headers.insert("authorization", "Bearer sk-dashboard-dev".parse().unwrap());
        let scope = resolve_scope(&state, &headers).await.expect("resolved");
        assert!(
            matches!(scope, Scope::Tenant(id) if id.to_string() == "22222222-2222-2222-2222-222222222222"),
            "tenant key wins over the session"
        );
        let mut headers = cookie_headers(&token);
        headers.insert("authorization", "Bearer op-secret".parse().unwrap());
        let scope = resolve_scope(&state, &headers).await.expect("resolved");
        assert!(matches!(scope, Scope::Operator), "operator key wins over the session");
    }

    // --- ITEM-037 AC1/AC4: operator sessions + isolation ---

    async fn operator_session_state() -> (std::sync::Arc<crate::AppState>, String) {
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let user = users.upsert_github_user(42, "octocat", true).await.expect("ok");
        let state = dashboard_state_full(Some(crate::auth::test_helpers::dummy_accounts(users)));
        let token = crate::session::issue_session(
            user.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        (state, token)
    }

    #[tokio::test]
    async fn an_operator_session_scopes_to_operator_without_a_membership() {
        let (state, token) = operator_session_state().await;
        let scope = resolve_scope(&state, &cookie_headers(&token)).await.expect("resolved");
        assert!(matches!(scope, Scope::Operator), "operator session -> platform scope");
    }

    #[tokio::test]
    async fn operator_and_tenant_sessions_stay_isolated_from_each_others_routes() {
        // An operator session is rejected by tenant-scoped gates (403).
        let (state, op_token) = operator_session_state().await;
        let err = tenant_or(&state, &cookie_headers(&op_token)).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::FORBIDDEN, "operator on tenant route");

        // A tenant-member session is rejected by operator gates (403).
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let member = users.upsert_github_user(7, "bob", false).await.expect("ok");
        users
            .add_membership(uuid::Uuid::new_v4(), member.id, "member")
            .await
            .expect("ok");
        let state = dashboard_state_full(Some(crate::auth::test_helpers::dummy_accounts(users)));
        let member_token = crate::session::issue_session(
            member.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        let err = operator_or(&state, &cookie_headers(&member_token)).await.unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::FORBIDDEN, "member on operator route");
    }

    #[tokio::test]
    async fn session_scoped_request_reaches_the_dashboard_handler() {
        // A session cookie alone must pass the route gate (the lazy pool makes
        // the SQL fail with 500, which proves the scope was granted — a 401
        // would mean the session was ignored).
        let (state, token, _) = signed_in_state().await;
        let router = crate::build_router(state);
        let request = Request::builder()
            .method("GET")
            .uri("/v1/dashboard/api/summary?days=7")
            .header(axum::http::header::COOKIE, format!("inferon_session={token}"))
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_ne!(
            response.status(),
            axum::http::StatusCode::UNAUTHORIZED,
            "session is not ignored at the route seam"
        );
        assert_ne!(response.status(), axum::http::StatusCode::FORBIDDEN);
    }

    // --- ITEM-037 AC3: the server-rendered operator console ---

    fn fixture_tenant(tenant_id: &str, cost: &str) -> ConsoleTenant {
        ConsoleTenant {
            tenant_id: tenant_id.to_string(),
            requests: 12,
            errors: 1,
            guard_fallbacks: 2,
            cost: cost.parse().unwrap(),
        }
    }

    #[test]
    fn console_html_renders_totals_tenants_guard_and_alerts() {
        let tenants = vec![
            fixture_tenant("11111111-1111-1111-1111-111111111111", "0.001234"),
            fixture_tenant("22222222-2222-2222-2222-222222222222", "0.000005"),
        ];
        let guard = vec![("fallback".to_string(), 2), ("flushed".to_string(), 5)];
        let alerts = vec![];
        let html = console_html(&tenants, &guard, &alerts, 7);
        assert!(html.contains("operator console"));
        assert!(html.contains("<b>2</b> tenants"), "tenant count");
        assert!(html.contains("24 requests"), "total requests (12+12)");
        assert!(html.contains("4 guard fallbacks"), "total fallbacks (2+2)");
        assert!(html.contains("0.001234"), "per-tenant spend as a decimal string");
        assert!(html.contains("11111111-1111-1111-1111-111111111111"));
        assert!(html.contains("guard fallbacks") && html.contains("flushed"), "guard view");
        assert!(html.contains("Alerts"), "alerts view present (empty state ok)");
        assert!(!html.contains("$1.6e-5"), "no float/scientific currency");
    }

    #[test]
    fn console_html_escapes_row_data() {
        // A hostile tenant id or alert payload must never inject markup.
        let tenants = vec![fixture_tenant("<script>alert(1)</script>", "0.001")];
        let alert = crate::alerts::AlertViewRow {
            tenant_id: uuid::Uuid::new_v4(),
            class: "spend".to_string(),
            status: "firing".to_string(),
            detail: serde_json::json!("<img src=x onerror=alert(1)>"),
            threshold: serde_json::json!({}),
            first_fired_at: "now".to_string(),
            last_fired_at: "now".to_string(),
            cleared_at: None,
        };
        let html = console_html(&tenants, &[], &[alert], 7);
        assert!(html.contains("&lt;script&gt;"), "tenant id escaped");
        assert!(!html.contains("<script>"), "no raw script tag");
        assert!(!html.contains("<img"), "no raw img/onerror tag");
        assert!(
            html.contains("&lt;img src=x onerror=alert(1)&gt;"),
            "alert payload escaped, not executable"
        );
    }

    #[tokio::test]
    async fn operator_console_route_is_gated_by_scope() {
        // No session/key -> 401; tenant-member session -> 403; operator key
        // reaches the handler (the lazy pool then 500s — 401/403 would mean the
        // gate let nobody through).
        let state = dashboard_state();
        let router = crate::build_router(state);
        let anon = router.clone().oneshot(get("/v1/dashboard/operator", None)).await.unwrap();
        assert_eq!(anon.status(), axum::http::StatusCode::UNAUTHORIZED, "no credential");
        let member = router
            .clone()
            .oneshot(get("/v1/dashboard/operator", Some("sk-dashboard-dev")))
            .await
            .unwrap();
        assert_eq!(member.status(), axum::http::StatusCode::FORBIDDEN, "tenant key rejected");
        let operator = router
            .clone()
            .oneshot(get("/v1/dashboard/operator", Some("op-secret")))
            .await
            .unwrap();
        assert_ne!(
            operator.status(),
            axum::http::StatusCode::UNAUTHORIZED,
            "operator key reaches the console handler"
        );
        assert_ne!(operator.status(), axum::http::StatusCode::FORBIDDEN);
    }

    // --- ITEM-038: tenant key management (session-scoped, owner/member) ---

    /// A session for a user with `role` on the seeded tenant 2222… over the
    /// seeded tenant store; returns (state, token).
    async fn tenant_role_session(role: &str) -> (std::sync::Arc<crate::AppState>, String) {
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let user = users.upsert_github_user(11, "t11", false).await.expect("ok");
        users
            .add_membership(uuid::Uuid::parse_str(SEEDED_TENANT).unwrap(), user.id, role)
            .await
            .expect("ok");
        let state = dashboard_state_full(Some(crate::auth::test_helpers::dummy_accounts(users)));
        let token = crate::session::issue_session(
            user.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        (state, token)
    }

    fn request_with_cookie(uri: &str, token: &str) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(uri)
            .header(axum::http::header::COOKIE, format!("inferon_session={token}"))
            .body(Body::empty())
            .unwrap()
    }

    fn post_with_cookie(uri: &str, token: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .header(axum::http::header::COOKIE, format!("inferon_session={token}"))
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    async fn body_json(response: axum::response::Response) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn owner_session_lists_creates_and_revokes_tenant_keys() {
        let (state, token) = tenant_role_session("owner").await;
        let router = crate::build_router(state);

        let listed = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/keys", &token))
            .await
            .unwrap();
        assert_eq!(listed.status(), axum::http::StatusCode::OK, "owner lists keys");
        let json: Value = body_json(listed).await;
        assert_eq!(json["role"], "owner");
        assert_eq!(json["keys"].as_array().unwrap().len(), 1, "the seeded dev key");

        let created = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/keys",
                &token,
                r#"{"label":"ci"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(created.status(), axum::http::StatusCode::OK);
        let created_json: Value = body_json(created).await;
        let raw = created_json["key"].as_str().expect("raw key shown once").to_string();
        let key_id = created_json["key_id"].as_str().expect("key id").to_string();
        assert!(raw.starts_with("sk-inferon-"));

        let revoke = router
            .clone()
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/keys/{key_id}/revoke"),
                &token,
                "",
            ))
            .await
            .unwrap();
        assert_eq!(revoke.status(), axum::http::StatusCode::OK, "owner revokes");
        let listed = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/keys", &token))
            .await
            .unwrap();
        let json: Value = body_json(listed).await;
        let ci = json["keys"]
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["label"] == "ci")
            .expect("ci key listed");
        assert_eq!(ci["revoked"], true, "revocation shows immediately");
    }

    #[tokio::test]
    async fn member_can_create_keys_but_revoking_is_403() {
        let (state, token) = tenant_role_session("member").await;
        let router = crate::build_router(state);
        let created = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/keys",
                &token,
                r#"{"label":"member-key"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(created.status(), axum::http::StatusCode::OK, "member may create");
        let json: Value = body_json(created).await;
        let key_id = json["key_id"].as_str().expect("key id").to_string();
        let revoke = router
            .clone()
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/keys/{key_id}/revoke"),
                &token,
                "",
            ))
            .await
            .unwrap();
        assert_eq!(
            revoke.status(),
            axum::http::StatusCode::FORBIDDEN,
            "members cannot revoke (AC2)"
        );
    }

    #[tokio::test]
    async fn keys_api_requires_a_human_session() {
        let state = dashboard_state();
        let router = crate::build_router(state);
        let none = router.clone().oneshot(get("/v1/dashboard/api/keys", None)).await.unwrap();
        assert_eq!(none.status(), axum::http::StatusCode::UNAUTHORIZED);
        let bearer = router
            .clone()
            .oneshot(get("/v1/dashboard/api/keys", Some("sk-dashboard-dev")))
            .await
            .unwrap();
        assert_eq!(bearer.status(), axum::http::StatusCode::UNAUTHORIZED, "bare key is not a human");
    }

    #[tokio::test]
    async fn tenant_revoke_cannot_kill_another_tenants_key() {
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let user = users.upsert_github_user(11, "t11", false).await.expect("ok");
        users
            .add_membership(uuid::Uuid::parse_str(SEEDED_TENANT).unwrap(), user.id, "owner")
            .await
            .expect("ok");
        // The store also holds a second tenant with its own key.
        let store = seeded_tenant_store();
        let other = store.insert_tenant(Some("other")).await.expect("ok");
        let other_key = store.insert_api_key(other, "other-key", "hash-x").await.expect("ok");
        let state = state_with(
            Some(crate::auth::test_helpers::dummy_accounts(users)),
            store,
        );
        let token = crate::session::issue_session(
            user.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        let router = crate::build_router(state);
        let revoke = router
            .clone()
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/keys/{other_key}/revoke"),
                &token,
                "",
            ))
            .await
            .unwrap();
        assert_eq!(
            revoke.status(),
            axum::http::StatusCode::NOT_FOUND,
            "another tenant's key id is unknown here"
        );
    }
}
