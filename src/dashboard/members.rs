//! The tenant membership surface.

use super::*;
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
