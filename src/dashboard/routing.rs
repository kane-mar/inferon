//! The tenant routing and budget/config surface.

use super::*;
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
        if model_is_disabled(&state, tenant_id, &model).await {
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
