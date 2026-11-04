//! Who is calling: a tenant key, the operator key, or a signed-in session.
//!
//! The two capabilities are independent — a platform operator who also belongs
//! to a tenant gets both views — which is why a caller is a small struct rather
//! than a single "is allowed" boolean.

use super::*;

/// A resolved dashboard caller.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Caller {
    /// The tenant this credential resolves to, if any.
    pub(crate) tenant: Option<uuid::Uuid>,
    /// True for the operator key or an operator session.
    pub(crate) is_operator: bool,
}

pub(crate) fn unauthorized() -> Response {
    crate::json_error(
        StatusCode::UNAUTHORIZED,
        "invalid_api_key",
        "missing or invalid Authorization header",
    )
}

pub(crate) fn forbidden() -> Response {
    crate::json_error(
        StatusCode::FORBIDDEN,
        "insufficient_role",
        "this action requires a tenant owner",
    )
}

/// Bearer-only resolution (ITEM-004/013 semantics, unchanged): operator key
/// first, then the tenant key lookup. `Ok(None)` when no usable credential was
/// presented — the caller may still fall through to the session path.
async fn bearer_scope(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<Option<Caller>, Response> {
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
        return Ok(Some(Caller {
            tenant: None,
            is_operator: true,
        }));
    }
    match crate::authenticate(state, headers).await {
        Ok(Some(tenant)) => Ok(Some(Caller {
            tenant: Some(tenant.id),
            is_operator: false,
        })),
        Ok(None) => Ok(None),
        Err(e) => {
            tracing::error!("tenant lookup failed: {e}");
            Err(internal("tenant lookup failed"))
        }
    }
}

/// The signed-in user behind the session cookie, if any.
///
/// One place verifies the cookie and loads the user: the tenant scope and the
/// membership check are the same session, and used to be decoded twice.
pub(crate) async fn session_user(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<Option<crate::auth::User>, Response> {
    let Some(accounts) = &state.accounts else {
        return Ok(None);
    };
    let Some(cookie) = headers.get("cookie").and_then(|v| v.to_str().ok()) else {
        return Ok(None);
    };
    let Some(token) = crate::session::cookie_value(cookie, crate::session::SESSION_COOKIE) else {
        return Ok(None);
    };
    let Some(user_id) = crate::session::verify_session(token, &accounts.session_secret, now_secs())
    else {
        return Ok(None);
    };
    match accounts.users.find_by_id(user_id).await {
        Ok(user) => Ok(user),
        Err(e) => {
            tracing::error!("user lookup failed: {e}");
            Err(internal("user lookup failed"))
        }
    }
}

/// The signed-in person behind a mutation, for the audit trail (ITEM-108).
pub(crate) struct SessionActor {
    pub(crate) tenant_id: uuid::Uuid,
    pub(crate) role: String,
    pub(crate) user_id: uuid::Uuid,
    pub(crate) login: String,
}

impl SessionActor {
    pub(crate) fn audit_actor(&self) -> crate::config_audit::Actor {
        crate::config_audit::Actor {
            id: self.user_id,
            login: self.login.clone(),
        }
    }
}

/// The session user and their membership in one lookup. Both the scope read
/// and the audit write need the same three facts; neither should look them up
/// twice or clone what it does not use.
async fn session_identity(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<(crate::auth::User, uuid::Uuid, String), Response> {
    let Some(user) = session_user(state, headers).await? else {
        return Err(unauthorized());
    };
    let Some(accounts) = &state.accounts else {
        return Err(unauthorized());
    };
    match accounts.users.tenant_for_user(user.id).await {
        Ok(Some((tenant_id, role))) => Ok((user, tenant_id, role)),
        Ok(None) => Err(unauthorized()),
        Err(e) => {
            tracing::error!("membership lookup failed: {e}");
            Err(internal("membership lookup failed"))
        }
    }
}

/// The session's tenant, role and user for a mutating handler.
pub(crate) async fn session_actor(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<SessionActor, Response> {
    let (user, tenant_id, role) = session_identity(state, headers).await?;
    Ok(SessionActor {
        tenant_id,
        role,
        user_id: user.id,
        login: user.github_login,
    })
}

/// Session-based resolution (ITEM-036 AC2): a signed-in owner/member reaches
/// their tenant scope through the session cookie — no API key needed. Missing,
/// forged, or expired cookies (or a memberless user) yield `Ok(None)`, never
/// an error.
async fn scope_from_session(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<Option<Caller>, Response> {
    let Some(user) = session_user(state, headers).await? else {
        return Ok(None);
    };
    let Some(accounts) = &state.accounts else {
        return Ok(None);
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
    Ok(Some(Caller {
        tenant,
        is_operator: user.is_operator,
    }))
}

/// Resolve the caller's scope: the bearer credential (operator key, then
/// tenant key — SDKs/scripts, ITEM-004) first, then the session cookie
/// (humans, ITEM-036). Additive: key auth is unchanged, sessions only add a
/// path.
pub(crate) async fn resolve_scope(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<Caller, Response> {
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
pub(crate) async fn tenant_or(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<uuid::Uuid, Response> {
    resolve_scope(state, headers)
        .await?
        .tenant
        .ok_or_else(forbidden)
}

/// The signed-in user's tenant membership — the human identity behind the
/// tenant-scoped management APIs (ITEM-038). Sessions only: a bare tenant key
/// is not a human identity (machines use the operator key API) and operator
/// sessions belong to the platform console, not a tenant role.
pub(crate) async fn session_membership(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<(uuid::Uuid, String), Response> {
    let (_, tenant_id, role) = session_identity(state, headers).await?;
    Ok((tenant_id, role))
}

/// Owner-gate for tenant management actions (ITEM-038/039): members can view
/// their tenant but never take destructive/owner actions.
pub(crate) fn owner_only(role: &str) -> Result<(), Response> {
    if role == "owner" {
        Ok(())
    } else {
        Err(forbidden())
    }
}
