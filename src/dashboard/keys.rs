//! The tenant API-key surface.

use super::*;
/// Key metadata as JSON — never key material or hashes (ITEM-034/038).
pub(crate) fn key_json(k: &crate::tenant::ApiKeyInfo) -> Value {
    json!({
        "id": k.id.to_string(),
        "label": k.label,
        "created_at": k.created_at,
        "revoked": k.revoked(),
        "revoked_at": k.revoked_at,
        "last_used_at": k.last_used_at,
        "spend_limit_cents": k.spend_limit_cents,
        "rpm_limit": k.rpm_limit,
        "tpm_limit": k.tpm_limit,
    })
}

/// The single mint implementation (ITEM-034 AC3 path, shared with the tenant
/// dashboard — ITEM-038 AC3): a fresh raw key shown once, hash-only stored.
pub(crate) async fn mint_key_payload(
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

/// GET /v1/dashboard/api/keys — the signed-in tenant's keys plus the caller's
/// role (ITEM-038 AC1). Never key material or hashes — lifecycle metadata only.
pub async fn tenant_keys(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    match state.tenant_store.list_api_keys(tenant_id).await {
        Ok(keys) => {
            let rows: Vec<Value> = keys.iter().map(key_json).collect();
            json_ok(json!({ "role": role, "keys": rows }))
        }
        Err(e) => internal(e.to_string()),
    }
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
    // ITEM-095: velocity limits ride the same owner-only endpoint.
    if let Err(e) = state
        .tenant_store
        .set_key_limits(tenant_id, key_id, body.rpm_limit, body.tpm_limit)
        .await
    {
        return internal(e.to_string());
    }
    match state
        .tenant_store
        .set_key_spend_limit(tenant_id, key_id, body.spend_limit_cents)
        .await
    {
        Ok(true) => json_ok(json!({
            "key_id": key_id.to_string(),
            "spend_limit_cents": body.spend_limit_cents.max(0),
            "rpm_limit": body.rpm_limit.max(0),
            "tpm_limit": body.tpm_limit.max(0),
        })),
        Ok(false) => crate::json_error(StatusCode::NOT_FOUND, "not_found", "no such key"),
        Err(e) => internal(e.to_string()),
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
        Ok(true) => {
            // ITEM-092: drop the cached identity so the revoked key stops now.
            state.hotpath.invalidate_key(key_id);
            state.hotpath.invalidate(tenant_id);
            json_ok(json!({ "key_id": key_id.to_string(), "revoked": true }))
        }
        Ok(false) => crate::json_error(
            StatusCode::NOT_FOUND,
            "key_not_found",
            "no such key for this tenant",
        ),
        Err(e) => internal(e.to_string()),
    }
}

#[derive(serde::Deserialize)]
pub struct KeyLabelBody {
    label: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct KeyLimitBody {
    spend_limit_cents: i64,
    /// ITEM-095: per-key velocity limits (0 = fall back to the tenant default).
    #[serde(default)]
    rpm_limit: i64,
    #[serde(default)]
    tpm_limit: i64,
}

