//! Dashboard data plane: read-only aggregate views over `traces`, always scoped
//! to the authenticated tenant (AC5). No new write path (AC3).
//!
//! Pure JSON APIs consumed by the TypeScript frontend (`web/`) — the backend
//! renders no HTML. Tenant views live under `/v1/dashboard/api/*`; platform-wide
//! operator views under `/v1/dashboard/api/operator/*`.

mod datasets;
mod performance;
mod sessions;
mod queries;
pub use datasets::*;
pub use performance::*;
pub use sessions::*;
pub use queries::*;

mod providers;
mod keys;
mod members;
mod audit_admin;
mod edge_admin;
mod routing;
pub use providers::*;
pub use keys::*;
pub use members::*;
pub use audit_admin::*;
pub use edge_admin::*;
pub use routing::*;

mod operator;
pub use operator::*;

use crate::clock::now_secs;
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

fn forbidden() -> Response {
    crate::json_error(
        StatusCode::FORBIDDEN,
        "forbidden",
        "this credential is not allowed here",
    )
}

/// A 500 that keeps the detail in the log and returns a stable message — an
/// SQL or storage error is never echoed back to a tenant.
fn internal(detail: impl std::fmt::Display) -> Response {
    tracing::error!("dashboard request failed: {detail}");
    crate::json_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "dashboard_error",
        "the request could not be completed",
    )
}

fn json_ok(value: Value) -> Response {
    Json(value).into_response()
}

fn db_err(e: sqlx::Error) -> Response {
    internal(e.to_string())
}

/// A resolved dashboard caller. The two capabilities are independent: a
/// platform operator who also belongs to a tenant gets BOTH the tenant view and
/// the platform view.
#[derive(Debug, PartialEq, Eq)]
struct Caller {
    /// The tenant this credential resolves to, if any.
    tenant: Option<uuid::Uuid>,
    /// True for the operator key or an operator session.
    is_operator: bool,
}

/// Bearer-only resolution (ITEM-004/013 semantics, unchanged): operator key
/// first, then the tenant key lookup. `Ok(None)` when no usable credential was
/// presented — the caller may still fall through to the session path.
async fn bearer_scope(state: &Arc<AppState>, headers: &HeaderMap) -> Result<Option<Caller>, Response> {
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
        return Ok(Some(Caller { tenant: None, is_operator: true }));
    }
    match crate::authenticate(state, headers).await {
        Ok(Some(tenant)) => Ok(Some(Caller { tenant: Some(tenant.id), is_operator: false })),
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
) -> Result<Option<Caller>, Response> {
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
    // Membership and operator flag are independent: an operator may also own
    // or belong to a tenant, and gets both views (ITEM-036 + ITEM-037).
    let tenant = match accounts.users.tenant_for_user(user.id).await {
        Ok(membership) => membership.map(|(tenant_id, _)| tenant_id),
        Err(e) => {
            tracing::error!("membership lookup failed: {e}");
            return Err(internal("membership lookup failed"));
        }
    };
    if !user.is_operator && tenant.is_none() {
        return Ok(None);
    }
    Ok(Some(Caller { tenant, is_operator: user.is_operator }))
}

/// Resolve the caller's scope: the bearer credential (operator key, then
/// tenant key — SDKs/scripts, ITEM-004) first, then the session cookie
/// (humans, ITEM-036). Additive: key auth is unchanged, sessions only add a
/// path.
async fn resolve_scope(state: &Arc<AppState>, headers: &HeaderMap) -> Result<Caller, Response> {
    if let Some(scope) = bearer_scope(state, headers).await? {
        return Ok(scope);
    }
    if let Some(scope) = scope_from_session(state, headers).await? {
        return Ok(scope);
    }
    Err(unauthorized())
}

/// Tenant-scoped handlers: need a tenant (key or membership). A platform
/// operator with no membership is rejected (403).
async fn tenant_or(state: &Arc<AppState>, headers: &HeaderMap) -> Result<uuid::Uuid, Response> {
    resolve_scope(state, headers).await?.tenant.ok_or_else(forbidden)
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
    match accounts.users.tenant_for_user(user.id).await {
        Ok(Some(membership)) => Ok(membership),
        Ok(None) => Err(unauthorized()),
        Err(e) => {
            tracing::error!("membership lookup failed: {e}");
            Err(internal("membership lookup failed"))
        }
    }
}

#[derive(serde::Deserialize)]
pub struct EdgeNodeBody {
    name: String,
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

#[derive(serde::Deserialize)]
pub struct KeyLabelBody {
    label: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct KeyLimitBody {
    spend_limit_cents: i64,
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

#[derive(serde::Deserialize)]
pub struct ProviderProbeBody {
    base_url: String,
    key: String,
    /// The kind being probed — decides the auth header + model-list surface
    /// (ITEM-050). Defaults to the OpenAI-compatible shape when absent.
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    metadata: crate::providers::ProviderMetadata,
}

// --- Models (ITEM-044, live from the provider) -------------------------------

#[derive(serde::Deserialize)]
pub struct ModelUpdateBody {
    enabled: Option<bool>,
    is_default: Option<bool>,
}

// --- Routing (ITEM-045) ------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct RoutingBody {
    fallbacks: Vec<String>,
}

#[derive(serde::Deserialize)]
pub struct TierPairsBody {
    tier_pairs: Vec<crate::router::TierPair>,
}

// --- Tenant controls (ITEM-046) ---------------------------------------------

#[derive(serde::Deserialize)]
pub struct ConfigBody {
    budget_limit_cents: i64,
    cheap_fallback_model: Option<String>,
    guard_enabled: bool,
    router_enabled: bool,
    router_threshold: f64,
    /// ITEM-088. Defaults keep an older client from turning capture off.
    #[serde(default = "default_true")]
    capture_payloads: bool,
    #[serde(default = "default_retention")]
    payload_retention_days: i64,
    /// ITEM-090. Defaults keep an older client from switching the lint off.
    #[serde(default = "default_true")]
    lint_enabled: bool,
}

fn default_true() -> bool {
    true
}

fn default_retention() -> i64 {
    30
}

#[derive(serde::Deserialize)]
pub struct MemberInviteBody {
    github_login: String,
}

#[cfg(test)]
mod tests;
