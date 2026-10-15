//! The tenant-facing edge-node admin surface (enroll/list/revoke).

use super::*;
/// A node as shown to the tenant (no token material).
fn edge_node_json(node: &crate::edge::EdgeNode) -> Value {
    json!({
        "id": node.id.to_string(),
        "name": node.name,
        "last_seen_at": node.last_seen_at,
        "created_at": node.created_at,
    })
}

/// GET /v1/dashboard/api/edge/nodes — the tenant's enrolled edge nodes.
pub async fn tenant_edge_nodes(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (tenant_id, _role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    match state.edge_store.list(tenant_id).await {
        Ok(nodes) => {
            json_ok(json!({ "nodes": nodes.iter().map(edge_node_json).collect::<Vec<_>>() }))
        }
        Err(e) => internal(&e.to_string()),
    }
}

/// POST /v1/dashboard/api/edge/nodes (owner-only) — enroll an edge node. The
/// token is returned exactly once; only its hash is stored.
pub async fn tenant_edge_node_create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<EdgeNodeBody>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    let name = body.name.trim().to_string();
    let name = if name.is_empty() {
        "edge node".to_string()
    } else {
        name
    };
    let token = crate::edge::mint_edge_token();
    match state
        .edge_store
        .enroll(tenant_id, &name, &crate::sha256_hex(&token))
        .await
    {
        Ok(id) => json_ok(json!({ "id": id.to_string(), "token": token })),
        Err(e) => internal(&e.to_string()),
    }
}

/// POST /v1/dashboard/api/edge/nodes/:node_id/revoke (owner-only).
pub async fn tenant_edge_node_revoke(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(node_id): Path<uuid::Uuid>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    match state.edge_store.revoke(tenant_id, node_id).await {
        Ok(true) => json_ok(json!({ "removed": true })),
        Ok(false) => crate::json_error(StatusCode::NOT_FOUND, "not_found", "no such edge node"),
        Err(e) => internal(&e.to_string()),
    }
}
