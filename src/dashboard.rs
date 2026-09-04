//! Cost-leak dashboard (ITEM-007): read-only aggregate views over `traces`,
//! always scoped to the authenticated tenant (AC5). No new write path (AC3).
//!
//! Data views are JSON APIs consumed by a small self-contained HTML page;
//! charts are CSS bars so there is no external asset or CDN dependency.

use crate::cost;
use crate::AppState;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
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
enum Scope {
    Tenant(uuid::Uuid),
    Operator,
}

/// Resolve the bearer key against the operator secret first, then the tenant
/// store (same seam as the proxy hot path). Operator access is disabled when
/// no OPERATOR_API_KEY is configured (ITEM-013).
async fn resolve_scope(state: &Arc<AppState>, headers: &HeaderMap) -> Result<Scope, Response> {
    let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) else {
        return Err(unauthorized());
    };
    let Some(key) = auth.strip_prefix("Bearer ") else {
        return Err(unauthorized());
    };
    if key.is_empty() {
        return Err(unauthorized());
    }
    if state.operator_key.as_deref() == Some(key) {
        return Ok(Scope::Operator);
    }
    match crate::authenticate(state, headers).await {
        Ok(Some(tenant)) => Ok(Scope::Tenant(tenant.id)),
        Ok(None) => Err(unauthorized()),
        Err(e) => {
            tracing::error!("tenant lookup failed: {e}");
            Err(internal("tenant lookup failed"))
        }
    }
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
</style>
</head>
<body>
<h1>Inferon — cost leaks</h1>
<p class="keyline">API key <input id="key" type="password" placeholder="sk-..." autocomplete="off">
  <button onclick="setDays(1)">1d</button><button onclick="setDays(7)">7d</button><button onclick="setDays(30)">30d</button>
  <span id="days"></span></p>
<div class="cards" id="cards"></div>
<h2>Spend by model</h2><div id="byModel"></div>
<h2>Top system prompts by spend</h2><div id="byPrompt"></div>
<h2>Waste flags &amp; estimated $ impact</h2><div id="waste"></div>
<p class="err" id="err"></p>
<script>
let days = 7;
const KEY = () => document.getElementById('key').value.trim();
const $ = id => document.getElementById(id);
function esc(s) { return String(s).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c])); }
function card(label, num) { return '<div class="card"><div>' + esc(label) + '</div><div class="num">' + esc(num) + '</div></div>'; }
function bars(items, key) {
  const max = Math.max(...items.map(i => parseFloat(i[key])), 0);
  return items.map(i => {
    const w = max > 0 ? Math.max(3, Math.round(100 * parseFloat(i[key]) / max)) : 0;
    return '<div class="bar" style="width:' + w + '%">' + esc(i.label) + ' — $' + esc(i[key]) + '</div>';
  }).join('') || '<p>no data</p>';
}
async function get(path) {
  const r = await fetch(path, { headers: { Authorization: 'Bearer ' + KEY() } });
  if (!r.ok) throw new Error((await r.json()).error?.message || r.status);
  return r.json();
}
async function load() {
  $('err').textContent = '';
  if (!KEY()) { $('err').textContent = 'enter your API key'; return; }
  try {
    const q = '?days=' + days;
    const s = await get('/v1/dashboard/api/summary' + q);
    $('days').textContent = 'last ' + s.window_days + ' days';
    $('cards').innerHTML =
      card('Total spend', '$' + s.total_cost_usd) +
      card('Requests', s.requests) +
      card('Errors (4xx/5xx)', s.errors) +
      card('Tenant', String(s.tenant_id).slice(0, 8) + '…');
    const m = await get('/v1/dashboard/api/by-model' + q);
    $('byModel').innerHTML = bars(m.models.map(x => ({ label: x.model, cost_usd: x.cost_usd })), 'cost_usd');
    const p = await get('/v1/dashboard/api/by-prompt' + q);
    $('byPrompt').innerHTML = bars(p.prompts.map(x => ({ label: x.system_prompt_hash.slice(0, 12), cost_usd: x.cost_usd })), 'cost_usd');
    const w = await get('/v1/dashboard/api/waste' + q);
    const names = { over_provisioned: 'Over-provisioned (gpt-4o, tiny output)', cache_miss: 'Cache-miss system prompts', model_misuse: 'gpt-4o on simple text' };
    $('waste').innerHTML = Object.keys(names).map(k => {
      const v = w[k];
      if (!v) return '';
      return '<div class="wasteflag"><b>' + names[k] + '</b>: ' + v.requests + ' requests, $' +
             v.spend_usd + ' spent' + (parseFloat(v.savings_est_usd) > 0 ? ' — ~$' + v.savings_est_usd + ' recoverable on gpt-4o-mini' : '') + '</div>';
    }).join('') || '<p>no waste flagged — clean traffic</p>';
  } catch (e) { $('err').textContent = 'error: ' + e.message; }
}
function setDays(d) { days = d; load(); }
document.getElementById('key').addEventListener('keydown', e => { if (e.key === 'Enter') load(); });
</script>
</body>
</html>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    /// Router state with an InMemory tenant store; `db` is a lazy pool that is
    /// never actually queried by the auth/index tests below.
    fn dashboard_state() -> std::sync::Arc<crate::AppState> {
        let mut tenants = std::collections::HashMap::new();
        let hash = {
            use sha2::{Digest, Sha256};
            hex::encode(Sha256::digest(b"sk-dashboard-dev"))
        };
        tenants.insert(
            hash,
            crate::Tenant {
                id: uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
                upstream_openai_key: Some("sk-master".to_string()),
            },
        );
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        std::sync::Arc::new(crate::AppState {
            http_client: reqwest::Client::new(),
            upstream_url: "http://127.0.0.1:1/".to_string(),
            tenant_store: std::sync::Arc::new(crate::InMemoryTenantStore::new(tenants)),
            telemetry_tx: tx,
            operator_key: Some("op-secret".to_string()),
            db: sqlx::PgPool::connect_lazy("postgres://inferon:inferon@localhost:5432/inferon")
                .unwrap(),
        })
    }

    fn get(uri: &str, key: Option<&str>) -> axum::http::Request<Body> {
        let mut builder = Request::builder().method("GET").uri(uri);
        if let Some(key) = key {
            builder = builder.header("authorization", format!("Bearer {key}"));
        }
        builder.body(Body::empty()).unwrap()
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
}
