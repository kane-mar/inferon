//! The tenant routing and budget/config surface.

use super::*;
use crate::router::{PairIssue, TierPair};

/// The most pairs a tenant may declare — the router keeps the first match, so a
/// long list is only ever a typo hazard.
const MAX_TIER_PAIRS: usize = 8;

/// GET /v1/dashboard/api/routing — the tenant's ordered fallback chain and its
/// tier pairs, each marked active only when both models can be routed to now.
pub async fn tenant_routing(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    let fallbacks = match state.routing_store.fallbacks(tenant_id).await {
        Ok(fallbacks) => fallbacks,
        Err(e) => return internal(e.to_string()),
    };
    let pairs = match state.budget_store.budget_for(tenant_id).await {
        Ok(Some(budget)) => budget.tier_pairs,
        Ok(None) => Vec::new(),
        Err(e) => return internal(e.to_string()),
    };
    let catalog = match model_catalog(&state, tenant_id).await {
        Ok(catalog) => catalog,
        Err(resp) => return resp,
    };
    json_ok(json!({
        "fallbacks": fallbacks,
        "tier_pairs": pair_json(&pairs, &catalog),
        "role": role,
    }))
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
        Err(e) => internal(e.to_string()),
    }
}

/// POST /v1/dashboard/api/routing/tier-pairs (owner-only) — replace the
/// tenant's own downshift pairs (ITEM-081). Refused pairs name the reason, so a
/// typo never becomes a rule that silently does nothing.
pub async fn tenant_tier_pairs_set(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<TierPairsBody>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    if body.tier_pairs.len() > MAX_TIER_PAIRS {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "tier_pairs_too_many",
            &format!("at most {MAX_TIER_PAIRS} tier pairs are allowed"),
        );
    }
    let pairs: Vec<TierPair> = body
        .tier_pairs
        .into_iter()
        .map(|pair| TierPair {
            frontier: pair.frontier.trim().to_string(),
            cheap: pair.cheap.trim().to_string(),
        })
        .collect();
    let catalog = match model_catalog(&state, tenant_id).await {
        Ok(catalog) => catalog,
        Err(resp) => return resp,
    };
    if let Err(issue) = crate::router::validate_pairs(&pairs, |model| catalog.knows(model)) {
        return pair_refusal(issue);
    }
    match state.budget_store.set_tier_pairs(tenant_id, &pairs).await {
        Ok(()) => json_ok(json!({ "tier_pairs": pair_json(&pairs, &catalog) })),
        Err(e) => internal(e.to_string()),
    }
}

/// The models the tenant can route to right now. `Open` for a tenant with no
/// providers: the shared platform key accepts any model, so there is nothing to
/// check against.
enum ModelCatalog {
    Open,
    Known(std::collections::HashSet<String>),
}

impl ModelCatalog {
    fn knows(&self, model: &str) -> bool {
        match self {
            ModelCatalog::Open => true,
            ModelCatalog::Known(models) => models.contains(model),
        }
    }
}

async fn model_catalog(
    state: &Arc<AppState>,
    tenant_id: uuid::Uuid,
) -> Result<ModelCatalog, Response> {
    let providers = state
        .provider_store
        .list(tenant_id)
        .await
        .map_err(|e| internal(e.to_string()))?;
    if providers.is_empty() {
        return Ok(ModelCatalog::Open);
    }
    let mut known = std::collections::HashSet::new();
    for provider in providers {
        let advertised = state
            .provider_store
            .advertised_models(provider.id)
            .await
            .map_err(|e| internal(e.to_string()))?;
        let settings = state
            .model_store
            .settings(provider.id)
            .await
            .map_err(|e| internal(e.to_string()))?;
        for model in advertised {
            let disabled = settings
                .iter()
                .any(|setting| setting.model_id == model && !setting.enabled);
            if !disabled {
                known.insert(model);
            }
        }
    }
    Ok(ModelCatalog::Known(known))
}

fn pair_json(pairs: &[TierPair], catalog: &ModelCatalog) -> Vec<Value> {
    pairs
        .iter()
        .map(|pair| {
            json!({
                "frontier": pair.frontier,
                "cheap": pair.cheap,
                "active": catalog.knows(&pair.frontier) && catalog.knows(&pair.cheap),
            })
        })
        .collect()
}

fn pair_refusal(issue: PairIssue) -> Response {
    let (kind, message) = match issue {
        PairIssue::EmptyModel => (
            "tier_pair_empty",
            "a tier pair needs both a frontier and a cheap model".to_string(),
        ),
        PairIssue::SameModel => (
            "tier_pair_same_model",
            "a tier pair needs two different models".to_string(),
        ),
        PairIssue::UnknownModel(model) => (
            "tier_pair_unknown_model",
            format!("`{model}` is not an enabled model for your tenant"),
        ),
        PairIssue::Duplicate => (
            "tier_pair_duplicate",
            "each frontier model may appear only once".to_string(),
        ),
        PairIssue::Cycle => (
            "tier_pair_cycle",
            "these pairs would loop; remove one of them".to_string(),
        ),
    };
    crate::json_error(StatusCode::BAD_REQUEST, kind, &message)
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
        Err(e) => return internal(e.to_string()),
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
        Err(e) => internal(e.to_string()),
    }
}
