//! The tenant-facing cost-audit shares (read-only links).

use super::*;
use crate::audit::AuditShareStore;

fn share_json(share: &crate::audit::AuditShare) -> Value {
    json!({
        "id": share.id.to_string(),
        "label": share.label,
        "created_at": share.created_at,
        "last_viewed_at": share.last_viewed_at,
        "revoked_at": share.revoked_at,
    })
}

/// GET /v1/dashboard/api/audit/shares — the tenant's share links.
pub async fn tenant_audit_shares(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (tenant_id, _role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    let store = crate::audit::PostgresAuditShareStore::new(state.db.clone());
    match store.list(tenant_id).await {
        Ok(shares) => json_ok(json!({
            "shares": shares.iter().map(share_json).collect::<Vec<_>>(),
        })),
        Err(e) => internal(&e),
    }
}

/// POST /v1/dashboard/api/audit/shares (owner-only) — mint a link. The token is
/// returned exactly once; only its hash is stored.
pub async fn tenant_audit_share_create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ShareLabelBody>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    let label = body.label.unwrap_or_default().trim().to_string();
    let token = crate::audit::mint_token();
    let store = crate::audit::PostgresAuditShareStore::new(state.db.clone());
    match store
        .create(tenant_id, &label, &crate::sha256_hex(&token))
        .await
    {
        Ok(id) => json_ok(json!({ "id": id.to_string(), "token": token })),
        Err(e) => internal(&e),
    }
}

/// POST /v1/dashboard/api/audit/shares/:id/revoke (owner-only).
pub async fn tenant_audit_share_revoke(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    let store = crate::audit::PostgresAuditShareStore::new(state.db.clone());
    match store.revoke(tenant_id, id).await {
        Ok(true) => json_ok(json!({ "id": id.to_string(), "revoked": true })),
        Ok(false) => crate::json_error(
            StatusCode::NOT_FOUND,
            "share_not_found",
            "no active share with that id",
        ),
        Err(e) => internal(&e),
    }
}

/// POST body: an optional human label for the link.
#[derive(serde::Deserialize)]
pub struct ShareLabelBody {
    pub label: Option<String>,
}
