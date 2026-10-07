//! Dashboard data plane: read-only aggregate views over `traces`, always scoped
//! to the authenticated tenant (AC5). No new write path (AC3).
//!
//! Pure JSON APIs consumed by the TypeScript frontend (`web/`) — the backend
//! renders no HTML. Tenant views live under `/v1/dashboard/api/*`; platform-wide
//! operator views under `/v1/dashboard/api/operator/*`.

use crate::cost;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::sync::Arc;

/// Query params shared by the JSON APIs: `days` (window, default 7, clamp
/// 1..=365), `limit` (row cap for by-prompt, default 10) and `offset` (days
/// back the window ends; 0 = the current window, used for period comparison).
#[derive(Debug, serde::Deserialize)]
pub struct Window {
    days: Option<i64>,
    limit: Option<i64>,
    offset: Option<i64>,
}

impl Window {
    fn days(&self) -> i64 {
        self.days.unwrap_or(7).clamp(1, 365)
    }
    fn limit(&self) -> i64 {
        self.limit.unwrap_or(10).clamp(1, 100)
    }
    /// Days back from now where the window ends (0 = current, `days` = the
    /// immediately-preceding period). Lets a caller diff two periods.
    fn offset(&self) -> i64 {
        self.offset.unwrap_or(0).clamp(0, 3650)
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
    let offset = window.offset();
    let row: Option<(i64, Decimal, i64, i64, i64, Option<f64>, Option<f64>, Option<String>, Option<String>)> =
        match sqlx::query_as(
            "SELECT COUNT(*),
                    COALESCE(SUM(upstream_cost_usd), 0),
                    COUNT(*) FILTER (WHERE status_code >= 400),
                    COALESCE(SUM(prompt_tokens), 0),
                    COALESCE(SUM(completion_tokens), 0),
                    AVG(time_to_first_token_ms)::float8,
                    AVG(total_duration_ms)::float8,
                    MIN(created_at)::text,
                    MAX(created_at)::text
             FROM traces
             WHERE ($1::uuid IS NULL OR tenant_id = $1)
               AND created_at >= NOW() - (($2 + $3) * INTERVAL '1 day')
               AND created_at < NOW() - ($3 * INTERVAL '1 day')",
        )
        .bind(tenant)
        .bind(days)
        .bind(offset)
        .fetch_optional(&state.db)
        .await
        {
            Ok(row) => row,
            Err(e) => return db_err(e),
        };

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
    ) = row.unwrap_or((0, Decimal::ZERO, 0, 0, 0, None, None, None, None));
    // Unit economics computed server-side so no float ever touches a cost.
    let avg_cost = if requests > 0 {
        (total_cost / Decimal::from(requests)).round_dp(8)
    } else {
        Decimal::ZERO
    };
    json_ok(json!({
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

/// ITEM-047: spend/usage grouped by provider over the window (offset shifts it
/// back for period comparison). A `NULL` provider_id is the shared platform key.
async fn query_by_provider(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
) -> Response {
    let days = window.days();
    let offset = window.offset();
    let rows: Vec<(Option<uuid::Uuid>, String, String, i64, i64, i64, Decimal, i64)> =
        match sqlx::query_as(
            "SELECT t.provider_id,
                    COALESCE(p.name, 'Shared platform key'),
                    COALESCE(p.kind, 'shared'),
                    COUNT(*),
                    COALESCE(SUM(t.prompt_tokens), 0),
                    COALESCE(SUM(t.completion_tokens), 0),
                    COALESCE(SUM(t.upstream_cost_usd), 0),
                    COALESCE(p.spend_limit_cents, 0)
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
        .await
        {
            Ok(rows) => rows,
            Err(e) => return db_err(e),
        };

    let providers: Vec<Value> = rows
        .into_iter()
        .map(
            |(id, name, kind, requests, prompt_tokens, completion_tokens, cost, limit_cents)| {
                json!({
                    "provider_id": id.map(|v| v.to_string()),
                    "name": name,
                    "kind": kind,
                    "requests": requests,
                    "prompt_tokens": prompt_tokens,
                    "completion_tokens": completion_tokens,
                    "cost_usd": cost.to_string(),
                    "spend_limit_cents": limit_cents,
                })
            },
        )
        .collect();
    json_ok(json!({
        "window_days": days,
        "offset_days": offset,
        "providers": providers,
    }))
}

/// ITEM-047: spend/usage grouped by API key over the window. A `NULL`
/// api_key_id is a legacy credential minted before per-key attribution.
async fn query_by_key(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
) -> Response {
    let days = window.days();
    let offset = window.offset();
    let rows: Vec<(Option<uuid::Uuid>, String, i64, i64, i64, Decimal, i64)> = match sqlx::query_as(
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
    .await
    {
        Ok(rows) => rows,
        Err(e) => return db_err(e),
    };

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
    json_ok(json!({
        "window_days": days,
        "offset_days": offset,
        "keys": keys,
    }))
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

/// Daily buckets over the window, gap-filled with zero rows so the chart is
/// continuous. `offset` shifts the window back (period comparison).
async fn query_timeseries(
    state: &Arc<AppState>,
    tenant: Option<uuid::Uuid>,
    window: &Window,
) -> Response {
    let days = window.days();
    let offset = window.offset();
    let rows: Vec<(String, i64, i64, Decimal, i64, i64, Option<f64>, Option<f64>)> =
        match sqlx::query_as(
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
        .await
        {
            Ok(rows) => rows,
            Err(e) => return db_err(e),
        };

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
    json_ok(json!({
        "window_days": days,
        "offset_days": offset,
        "points": points,
    }))
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

/// GET /v1/dashboard/api/by-provider?days=7 (tenant scope)
pub async fn by_provider(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    match tenant_or(&state, &headers).await {
        Ok(id) => query_by_provider(&state, Some(id), &window).await,
        Err(resp) => resp,
    }
}

/// GET /v1/dashboard/api/by-key?days=7 (tenant scope)
pub async fn by_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    match tenant_or(&state, &headers).await {
        Ok(id) => query_by_key(&state, Some(id), &window).await,
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

/// GET /v1/dashboard/api/timeseries?days=30&offset=0 (tenant scope)
pub async fn timeseries(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    match tenant_or(&state, &headers).await {
        Ok(id) => query_timeseries(&state, Some(id), &window).await,
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
                        "spend_limit_cents": k.spend_limit_cents,
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
    let hash = crate::sha256_hex(&raw);
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
                        "spend_limit_cents": k.spend_limit_cents,
                    })
                })
                .collect();
            json_ok(json!({ "role": role, "keys": rows }))
        }
        Err(e) => internal(&e.to_string()),
    }
}

#[derive(serde::Deserialize)]
pub struct KeyLimitBody {
    spend_limit_cents: i64,
}

/// POST /v1/dashboard/api/keys/:key_id/limit (owner-only, ITEM-047) — set a
/// tenant-owned key's monthly spend cap (0 clears it).
pub async fn tenant_key_limit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_id): Path<uuid::Uuid>,
    Json(body): Json<KeyLimitBody>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    match state
        .tenant_store
        .set_key_spend_limit(tenant_id, key_id, body.spend_limit_cents)
        .await
    {
        Ok(true) => json_ok(json!({
            "key_id": key_id.to_string(),
            "spend_limit_cents": body.spend_limit_cents.max(0),
        })),
        Ok(false) => crate::json_error(StatusCode::NOT_FOUND, "not_found", "no such key"),
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

/// Owner-gate for tenant management actions (ITEM-038/039): members can view
/// their tenant but never take destructive/owner actions.
fn owner_only(role: &str) -> Result<(), Response> {
    if role == "owner" {
        Ok(())
    } else {
        Err(forbidden())
    }
}

// --- Providers (ITEM-043) ----------------------------------------------------

#[derive(serde::Deserialize)]
pub struct ProviderBody {
    kind: String,
    name: Option<String>,
    base_url: String,
    key: String,
    #[serde(default)]
    metadata: crate::providers::ProviderMetadata,
    #[serde(default)]
    spend_limit_cents: Option<i64>,
}

#[derive(serde::Deserialize)]
pub struct ProviderUpdateBody {
    name: Option<String>,
    base_url: Option<String>,
    key: Option<String>,
    #[serde(default)]
    metadata: Option<crate::providers::ProviderMetadata>,
    #[serde(default)]
    spend_limit_cents: Option<i64>,
}

fn provider_json(provider: &crate::providers::Provider) -> Value {
    json!({
        "id": provider.id.to_string(),
        "kind": provider.kind,
        "name": provider.name,
        "base_url": provider.base_url,
        "key_hint": provider.key_hint,
        "metadata": provider.metadata,
        "spend_limit_cents": provider.spend_limit_cents,
        "created_at": provider.created_at,
        "updated_at": provider.updated_at,
    })
}

/// GET /v1/dashboard/api/providers (session-scoped) — masked provider list.
pub async fn tenant_providers(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    match state.provider_store.list(tenant_id).await {
        Ok(providers) => json_ok(json!({
            "providers": providers.iter().map(provider_json).collect::<Vec<_>>(),
            "kinds": crate::providers::PROVIDER_KINDS,
            "role": role,
        })),
        Err(e) => internal(&e.to_string()),
    }
}

/// POST /v1/dashboard/api/providers (owner-only) — register a provider.
pub async fn tenant_provider_create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ProviderBody>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    if !crate::providers::is_supported_kind(&body.kind) {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "unsupported_kind",
            "that provider kind is not supported yet",
        );
    }
    let base_url = body.base_url.trim().trim_end_matches('/').to_string();
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "invalid_base_url",
            "base URL must start with http:// or https://",
        );
    }
    let key = body.key.trim().to_string();
    if key.is_empty() {
        return crate::json_error(StatusCode::BAD_REQUEST, "invalid_key", "a provider key is required");
    }
    let name = body
        .name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| crate::providers::default_name(&body.kind));
    let new = crate::providers::NewProvider {
        kind: body.kind,
        name,
        base_url,
        key,
        metadata: body.metadata,
        spend_limit_cents: body.spend_limit_cents.unwrap_or(0),
    };
    match state.provider_store.insert(tenant_id, &new).await {
        Ok(id) => json_ok(json!({ "id": id.to_string() })),
        Err(e) => {
            tracing::error!("provider insert failed: {e}");
            internal(&e.to_string())
        }
    }
}

/// POST /v1/dashboard/api/providers/:provider_id/remove (owner-only).
pub async fn tenant_provider_remove(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(provider_id): Path<uuid::Uuid>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    match state.provider_store.remove(tenant_id, provider_id).await {
        Ok(removed) => json_ok(json!({
            "provider_id": provider_id.to_string(),
            "removed": removed,
        })),
        Err(e) => internal(&e.to_string()),
    }
}

/// POST /v1/dashboard/api/providers/:provider_id (owner-only) — rotate/update a
/// provider in place (ITEM-043 AC5). Omitted fields are left unchanged.
pub async fn tenant_provider_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(provider_id): Path<uuid::Uuid>,
    Json(body): Json<ProviderUpdateBody>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    let base_url = match body.base_url {
        Some(url) => {
            let url = url.trim().trim_end_matches('/').to_string();
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return crate::json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_base_url",
                    "base URL must start with http:// or https://",
                );
            }
            Some(url)
        }
        None => None,
    };
    let key = body
        .key
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty());
    let update = crate::providers::ProviderUpdate {
        name: body
            .name
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty()),
        base_url,
        key,
        metadata: body.metadata,
        spend_limit_cents: body.spend_limit_cents,
    };
    match state.provider_store.update(tenant_id, provider_id, &update).await {
        Ok(true) => json_ok(json!({ "provider_id": provider_id.to_string(), "updated": true })),
        Ok(false) => {
            crate::json_error(StatusCode::NOT_FOUND, "not_found", "no such provider")
        }
        Err(e) => internal(&e.to_string()),
    }
}

/// POST /v1/dashboard/api/providers/:provider_id/test — a minimal probe of the
/// provider's base URL + key (`GET <base>/models`, the OpenAI-compatible
/// endpoint; costs nothing). Reports `{ ok, status }` — `status` is `null` when
/// the provider could not be reached.
pub async fn tenant_provider_test(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(provider_id): Path<uuid::Uuid>,
) -> Response {
    let (tenant_id, _role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    let provider = match state.provider_store.resolve_provider(tenant_id, provider_id).await {
        Ok(Some(provider)) => provider,
        Ok(None) => {
            return crate::json_error(StatusCode::NOT_FOUND, "not_found", "no such provider")
        }
        Err(e) => return internal(&e.to_string()),
    };
    let url = format!("{}/models", provider.base_url.trim_end_matches('/'));
    let mut probe = state
        .http_client
        .get(&url)
        .header("Authorization", format!("Bearer {}", provider.api_key))
        .timeout(std::time::Duration::from_secs(10));
    if let Some(org) = &provider.metadata.org {
        probe = probe.header("OpenAI-Organization", org);
    }
    for (header, value) in &provider.metadata.headers {
        probe = probe.header(header, value);
    }
    let probe = probe.send().await;
    match probe {
        Ok(response) => json_ok(json!({
            "ok": response.status().is_success(),
            "status": response.status().as_u16(),
        })),
        Err(e) => {
            tracing::warn!("provider probe failed for {provider_id}: {e}");
            json_ok(json!({ "ok": false, "status": null }))
        }
    }
}

// --- Models (ITEM-044) -------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct ModelUpdateBody {
    enabled: Option<bool>,
    is_default: Option<bool>,
}

/// GET /v1/dashboard/api/models — the catalog for the tenant's provider kinds,
/// merged with the tenant's enable/disable + default flags. Prices come from the
/// cost engine (`cost::price_for`) so the two can never disagree.
pub async fn tenant_models(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    let providers = match state.provider_store.list(tenant_id).await {
        Ok(providers) => providers,
        Err(e) => return internal(&e.to_string()),
    };
    let settings = match state.model_store.settings(tenant_id).await {
        Ok(settings) => settings,
        Err(e) => return internal(&e.to_string()),
    };

    let mut kinds: Vec<&str> = providers.iter().map(|provider| provider.kind.as_str()).collect();
    kinds.sort_unstable();
    kinds.dedup();

    let models: Vec<Value> = kinds
        .iter()
        .flat_map(|kind| crate::models::by_kind(kind))
        .map(|model| {
            let setting = settings.iter().find(|setting| setting.model_id == model.id);
            let price = crate::cost::price_for(model.id);
            json!({
                "id": model.id,
                "kind": model.kind,
                "name": model.name,
                "context_window": model.context_window,
                "max_output": model.max_output,
                "reasoning": model.reasoning,
                "images": model.images,
                "enabled": setting.map(|setting| setting.enabled).unwrap_or(true),
                "is_default": setting.map(|setting| setting.is_default).unwrap_or(false),
                "price": {
                    "input_usd_per_million": price.prompt_miss.to_string(),
                    "cached_input_usd_per_million": price.prompt_hit.map(|rate| rate.to_string()),
                    "output_usd_per_million": price.completion.to_string(),
                },
            })
        })
        .collect();
    json_ok(json!({ "models": models, "role": role }))
}

/// POST /v1/dashboard/api/models/:model_id (owner-only) — enable/disable a model
/// and/or make it the default for its provider kind.
pub async fn tenant_model_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(model_id): Path<String>,
    Json(body): Json<ModelUpdateBody>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    if let Some(enabled) = body.enabled {
        if let Err(e) = state.model_store.set_enabled(tenant_id, &model_id, enabled).await {
            return internal(&e.to_string());
        }
    }
    if body.is_default == Some(true) {
        if let Err(e) = state.model_store.set_default(tenant_id, &model_id).await {
            return internal(&e.to_string());
        }
    }
    json_ok(json!({ "model_id": model_id }))
}

// --- Routing (ITEM-045) ------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct RoutingBody {
    fallbacks: Vec<String>,
}

/// GET /v1/dashboard/api/routing — the tenant's ordered fallback chain.
pub async fn tenant_routing(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    match state.routing_store.fallbacks(tenant_id).await {
        Ok(fallbacks) => json_ok(json!({ "fallbacks": fallbacks, "role": role })),
        Err(e) => internal(&e.to_string()),
    }
}

/// POST /v1/dashboard/api/routing (owner-only) — set the ordered chain. The
/// chain is capped and a disabled model is refused up front (ITEM-044).
pub async fn tenant_routing_set(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<RoutingBody>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    let mut chain: Vec<String> = Vec::new();
    for model in body.fallbacks.into_iter().take(8) {
        let model = model.trim().to_string();
        if model.is_empty() || chain.contains(&model) {
            continue;
        }
        if state
            .model_store
            .is_disabled(tenant_id, &model)
            .await
            .unwrap_or(false)
        {
            return crate::json_error(
                StatusCode::BAD_REQUEST,
                "model_disabled",
                "a fallback model is disabled for your tenant",
            );
        }
        chain.push(model);
    }
    match state.routing_store.set_fallbacks(tenant_id, &chain).await {
        Ok(()) => json_ok(json!({ "fallbacks": chain })),
        Err(e) => internal(&e.to_string()),
    }
}

// --- Tenant controls (ITEM-046) ---------------------------------------------

#[derive(serde::Deserialize)]
pub struct ConfigBody {
    budget_limit_cents: i64,
    cheap_fallback_model: Option<String>,
    guard_enabled: bool,
    router_enabled: bool,
    router_threshold: f64,
}

/// GET /v1/dashboard/api/config — the tenant's routing/budget controls, with the
/// current period's spend against the cap.
pub async fn tenant_config(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    let config = match state.budget_store.budget_for(tenant_id).await {
        Ok(config) => config,
        Err(e) => return internal(&e.to_string()),
    };
    let (limit, spent, cheap, guard, router_enabled, router_threshold) = match config {
        Some(budget) => (
            budget.limit_cents,
            budget.spent_cents,
            budget.cheap_fallback_model,
            budget.guard_enabled,
            budget.router_enabled,
            budget.router_threshold,
        ),
        // Unconfigured: the router/guard are on by default (ITEM-015/027).
        None => (0, 0, None, true, true, 0.35),
    };
    json_ok(json!({
        "budget_limit_cents": limit,
        "current_spend_cents": spent,
        "cheap_fallback_model": cheap,
        "guard_enabled": guard,
        "router_enabled": router_enabled,
        "router_threshold": router_threshold,
        "role": role,
    }))
}

/// POST /v1/dashboard/api/config (owner-only) — update the controls.
pub async fn tenant_config_set(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ConfigBody>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    let update = crate::budget::ConfigUpdate {
        budget_limit_cents: body.budget_limit_cents.max(0),
        cheap_fallback_model: body
            .cheap_fallback_model
            .map(|model| model.trim().to_string())
            .filter(|model| !model.is_empty()),
        guard_enabled: body.guard_enabled,
        router_enabled: body.router_enabled,
        router_threshold: body.router_threshold.clamp(0.0, 1.0),
    };
    match state.budget_store.update_config(tenant_id, &update).await {
        Ok(()) => json_ok(json!({ "updated": true })),
        Err(e) => internal(&e.to_string()),
    }
}

/// GET /v1/dashboard/api/members — the signed-in tenant's members + pending
/// invites (ITEM-039). Owners and members may view.
pub async fn tenant_members(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    let users = state
        .accounts
        .as_ref()
        .expect("a resolved session implies accounts")
        .users
        .clone();
    let members = match users.list_members(tenant_id).await {
        Ok(rows) => rows,
        Err(e) => return internal(&e.to_string()),
    };
    let invites = match users.list_invites(tenant_id).await {
        Ok(rows) => rows,
        Err(e) => return internal(&e.to_string()),
    };
    let rows: Vec<Value> = members
        .into_iter()
        .map(|(id, login, mrole)| {
            json!({ "id": id.to_string(), "github_login": login, "role": mrole })
        })
        .collect();
    json_ok(json!({ "role": role, "members": rows, "invites": invites }))
}

#[derive(serde::Deserialize)]
pub struct MemberInviteBody {
    github_login: String,
}

/// POST /v1/dashboard/api/members — the owner invites a GitHub login to the
/// tenant as member (ITEM-039 AC1). Claimed when that user signs in.
pub async fn tenant_member_invite(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<MemberInviteBody>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    let login = body.github_login.trim().to_string();
    if login.is_empty() {
        return crate::json_error(StatusCode::BAD_REQUEST, "bad_request", "login required");
    }
    match state
        .accounts
        .as_ref()
        .expect("session implies accounts")
        .users
        .invite_member(tenant_id, &login)
        .await
    {
        Ok(()) => json_ok(json!({ "github_login": login, "invited": true })),
        Err(e) => internal(&e.to_string()),
    }
}

/// POST /v1/dashboard/api/members/{user_id}/remove — the owner removes a
/// member (their scope ends on the next request). The tenant owner cannot be
/// removed.
pub async fn tenant_member_remove(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<uuid::Uuid>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    match state
        .accounts
        .as_ref()
        .expect("session implies accounts")
        .users
        .remove_member(tenant_id, user_id)
        .await
    {
        Ok(true) => json_ok(json!({ "user_id": user_id.to_string(), "removed": true })),
        Ok(false) => crate::json_error(
            StatusCode::BAD_REQUEST,
            "member_not_removable",
            "no such member, or the tenant owner cannot be removed",
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
                api_key_id: None,
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
            provider_store: std::sync::Arc::new(crate::providers::InMemoryProviderStore::default()),
            model_store: std::sync::Arc::new(crate::models::InMemoryModelStore::default()),
            routing_store: std::sync::Arc::new(crate::routing::InMemoryRoutingStore::default()),
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
    async fn dashboard_apis_require_a_valid_tenant_key() {
        let state = dashboard_state();
        let router = crate::build_router(state);
        for uri in [
            "/v1/dashboard/api/summary",
            "/v1/dashboard/api/by-model",
            "/v1/dashboard/api/by-prompt",
            "/v1/dashboard/api/by-provider",
            "/v1/dashboard/api/by-key",
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
    async fn owner_sets_a_key_spend_limit_and_members_cannot() {
        // ITEM-047 AC2: per-key limits are owner-managed and reflected in the
        // key list; a member gets 403.
        let (state, token) = tenant_role_session("owner").await;
        let router = crate::build_router(state);
        let listed = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/keys", &token))
            .await
            .unwrap();
        let json: Value = body_json(listed).await;
        let key_id = json["keys"][0]["id"].as_str().expect("seeded key id").to_string();

        let set = router
            .clone()
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/keys/{key_id}/limit"),
                &token,
                r#"{"spend_limit_cents":2500}"#,
            ))
            .await
            .unwrap();
        assert_eq!(set.status(), axum::http::StatusCode::OK);
        assert_eq!(body_json(set).await["spend_limit_cents"], 2500);

        let listed = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/keys", &token))
            .await
            .unwrap();
        let json: Value = body_json(listed).await;
        assert_eq!(json["keys"][0]["spend_limit_cents"], 2500);

        let (member_state, member_token) = tenant_role_session("member").await;
        let member = crate::build_router(member_state)
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/keys/{key_id}/limit"),
                &member_token,
                r#"{"spend_limit_cents":1}"#,
            ))
            .await
            .unwrap();
        assert_eq!(member.status(), axum::http::StatusCode::FORBIDDEN);
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
        let w = |days, limit, offset| Window { days, limit, offset };
        assert_eq!(w(None, None, None).days(), 7);
        assert_eq!(w(Some(3), None, None).days(), 3);
        assert_eq!(w(Some(9999), None, None).days(), 365);
        assert_eq!(w(Some(0), None, None).days(), 1);
        assert_eq!(w(None, Some(500), None).limit(), 100);
        assert_eq!(w(None, Some(3), None).limit(), 3);
        // `offset` shifts the window back for period comparison.
        assert_eq!(w(None, None, None).offset(), 0);
        assert_eq!(w(None, None, Some(30)).offset(), 30);
        assert_eq!(w(None, None, Some(-5)).offset(), 0);
        assert_eq!(w(None, None, Some(99_999)).offset(), 3650);
    }

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

    // --- ITEM-046: routing & budget controls (session-scoped, owner mutates) ---

    #[tokio::test]
    async fn owner_reads_and_updates_the_tenant_controls() {
        let (state, token) = tenant_role_session("owner").await;
        let router = crate::build_router(state);

        // Unconfigured tenant: router + guard on by default, no budget cap.
        let read = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/config", &token))
            .await
            .unwrap();
        assert_eq!(read.status(), axum::http::StatusCode::OK);
        let json: Value = body_json(read).await;
        assert_eq!(json["budget_limit_cents"], 0);
        assert_eq!(json["current_spend_cents"], 0);
        assert_eq!(json["guard_enabled"], true);
        assert_eq!(json["router_enabled"], true);
        assert_eq!(json["router_threshold"], 0.35);
        assert_eq!(json["role"], "owner");

        // Owner writes the controls; they read back unchanged.
        let updated = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/config",
                &token,
                r#"{"budget_limit_cents":2500,"cheap_fallback_model":"gpt-4o-mini","guard_enabled":false,"router_enabled":false,"router_threshold":0.8}"#,
            ))
            .await
            .unwrap();
        assert_eq!(updated.status(), axum::http::StatusCode::OK);
        assert_eq!(body_json(updated).await["updated"], true);

        let read = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/config", &token))
            .await
            .unwrap();
        let json: Value = body_json(read).await;
        assert_eq!(json["budget_limit_cents"], 2500);
        assert_eq!(json["cheap_fallback_model"], "gpt-4o-mini");
        assert_eq!(json["guard_enabled"], false);
        assert_eq!(json["router_enabled"], false);
        assert_eq!(json["router_threshold"], 0.8);
    }

    #[tokio::test]
    async fn member_cannot_update_the_tenant_controls() {
        let (state, token) = tenant_role_session("member").await;
        let router = crate::build_router(state);
        let read = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/config", &token))
            .await
            .unwrap();
        assert_eq!(read.status(), axum::http::StatusCode::OK, "members may view");
        let write = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/config",
                &token,
                r#"{"budget_limit_cents":1,"cheap_fallback_model":null,"guard_enabled":true,"router_enabled":true,"router_threshold":0.35}"#,
            ))
            .await
            .unwrap();
        assert_eq!(write.status(), axum::http::StatusCode::FORBIDDEN);
    }

    // --- ITEM-043: provider rotation/metadata (session-scoped, owner mutates) ---

    #[tokio::test]
    async fn owner_rotates_a_provider_and_members_cannot() {
        let (state, token) = tenant_role_session("owner").await;
        let tenant_id = uuid::Uuid::parse_str(SEEDED_TENANT).unwrap();
        let id = state
            .provider_store
            .insert(
                tenant_id,
                &crate::providers::NewProvider {
                    kind: "openai".to_string(),
                    name: "Mine".to_string(),
                    base_url: "https://api.openai.com/v1".to_string(),
                    key: "sk-old-1111".to_string(),
                    metadata: Default::default(),
                    spend_limit_cents: 0,
                },
            )
            .await
            .expect("seed");
        let router = crate::build_router(state);

        // The list exposes the last-updated date + metadata (AC2).
        let listed = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/providers", &token))
            .await
            .unwrap();
        assert_eq!(listed.status(), axum::http::StatusCode::OK);
        let json: Value = body_json(listed).await;
        assert!(json["providers"][0]["updated_at"].is_string());
        assert!(json["providers"][0]["metadata"].is_object());

        // Rotate the key + attach org/headers (AC1/AC5).
        let updated = router
            .clone()
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/providers/{id}"),
                &token,
                r#"{"key":"sk-new-2222","metadata":{"org":"org-2","headers":{"X-Trace":"on"}}}"#,
            ))
            .await
            .unwrap();
        assert_eq!(updated.status(), axum::http::StatusCode::OK);
        assert_eq!(body_json(updated).await["updated"], true);

        let listed = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/providers", &token))
            .await
            .unwrap();
        let json: Value = body_json(listed).await;
        assert_eq!(json["providers"][0]["key_hint"], "\u{2026}2222");
        assert_eq!(json["providers"][0]["metadata"]["org"], "org-2");
        assert_eq!(json["providers"][0]["metadata"]["headers"]["X-Trace"], "on");

        // Unknown id -> 404; a member cannot rotate at all.
        let missing = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/providers/99999999-9999-9999-9999-999999999999",
                &token,
                r#"{"key":"sk-x-1234"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(missing.status(), axum::http::StatusCode::NOT_FOUND);
        let (member_state, member_token) = tenant_role_session("member").await;
        let member = crate::build_router(member_state)
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/providers/{id}"),
                &member_token,
                r#"{"key":"sk-x-1234"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(member.status(), axum::http::StatusCode::FORBIDDEN);
    }

    // --- ITEM-039: tenant members API (session-scoped, owner manages) ---

    /// The members endpoint (ITEM-039) is scoped to the signed-in user's own
    /// tenant. These tests use owner/member sessions on the seeded tenant.

    #[tokio::test]
    async fn owner_invites_lists_and_removes_members() {
        let (state, token) = owner_with_member_state().await;
        let router = crate::build_router(state);

        // Owner invites a teammate by GitHub login (claim-based).
        let invite = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/members",
                &token,
                r#"{"github_login":"carol"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(invite.status(), axum::http::StatusCode::OK, "owner invites");

        // The members view shows owner + member + the pending invite.
        let view = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/members", &token))
            .await
            .unwrap();
        assert_eq!(view.status(), axum::http::StatusCode::OK);
        let json: Value = body_json(view).await;
        assert_eq!(json["invites"], serde_json::json!(["carol"]));
        let logins: Vec<&str> = json["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["github_login"].as_str().unwrap())
            .collect();
        assert_eq!(logins, vec!["t11", "alice"], "owner + member listed");
        let alice_id = json["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["github_login"] == "alice")
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        // Owner removes alice; her membership (and invite, if any) is gone.
        let remove = router
            .clone()
            .oneshot(post_with_cookie(
                &format!("/v1/dashboard/api/members/{alice_id}/remove"),
                &token,
                "",
            ))
            .await
            .unwrap();
        assert_eq!(remove.status(), axum::http::StatusCode::OK);
        let view = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/members", &token))
            .await
            .unwrap();
        let json: Value = body_json(view).await;
        let logins: Vec<&str> = json["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["github_login"].as_str().unwrap())
            .collect();
        assert_eq!(logins, vec!["t11"], "alice removed");
    }

    /// Owner t11 + member alice on the seeded tenant 2222….
    async fn owner_with_member_state() -> (std::sync::Arc<crate::AppState>, String) {
        let users: std::sync::Arc<dyn crate::auth::UserStore> =
            std::sync::Arc::new(crate::auth::InMemoryUserStore::default());
        let owner = users.upsert_github_user(11, "t11", false).await.expect("ok");
        let alice = users.upsert_github_user(21, "alice", false).await.expect("ok");
        let tid = uuid::Uuid::parse_str(SEEDED_TENANT).unwrap();
        users.add_membership(tid, owner.id, "owner").await.expect("ok");
        users.add_membership(tid, alice.id, "member").await.expect("ok");
        let state = dashboard_state_full(Some(crate::auth::test_helpers::dummy_accounts(users)));
        let token = crate::session::issue_session(
            owner.id,
            crate::auth::test_helpers::TEST_SESSION_SECRET,
            now_secs(),
            3600,
        );
        (state, token)
    }

    #[tokio::test]
    async fn member_sessions_view_members_but_cannot_invite_or_remove() {
        let (state, token) = tenant_role_session("member").await;
        let router = crate::build_router(state);
        let view = router
            .clone()
            .oneshot(request_with_cookie("/v1/dashboard/api/members", &token))
            .await
            .unwrap();
        assert_eq!(view.status(), axum::http::StatusCode::OK, "members may view");
        let invite = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/members",
                &token,
                r#"{"github_login":"bob"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(
            invite.status(),
            axum::http::StatusCode::FORBIDDEN,
            "members cannot invite (owner only)"
        );
        let remove = router
            .clone()
            .oneshot(post_with_cookie(
                "/v1/dashboard/api/members/00000000-0000-0000-0000-000000000000/remove",
                &token,
                "",
            ))
            .await
            .unwrap();
        assert_eq!(remove.status(), axum::http::StatusCode::FORBIDDEN);
    }
}
