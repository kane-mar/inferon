// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;

/// The resolved upstream target for one request (ITEM-043): the tenant's own
/// provider that owns the requested model, else its first provider, else the
/// shared key + the global URL.
#[derive(Clone)]
pub(crate) struct Route {
    pub(crate) url: String,
    pub(crate) key: String,
    /// Extra upstream headers from the tenant's provider (ITEM-043 AC1).
    pub(crate) metadata: providers::ProviderMetadata,
    /// ITEM-047: the provider row that served the request (`None` = shared key).
    pub(crate) provider_id: Option<Uuid>,
    /// ITEM-050: the provider kind, which decides the upstream auth + surface.
    pub(crate) kind: String,
    /// ITEM-077: a locally hosted provider — costed at zero.
    pub(crate) is_local: bool,
    /// ITEM-094: the surface the provider speaks when it is not the client's.
    /// The body and the response are translated across this seam.
    pub(crate) translate: Option<Surface>,
    /// ITEM-100: why this provider was chosen (`only`, `weight`, `round_robin`).
    pub(crate) selection: Option<&'static str>,
}

/// The ingress→upstream surface (ITEM-050): OpenAI is the chat-completions
/// shape, Anthropic the Messages shape. A request never crosses surfaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Surface {
    OpenAi,
    Anthropic,
}

impl Surface {
    /// `POST <base>/chat/completions` or `POST <base>/messages`.
    pub(crate) fn endpoint(self, base_url: &str) -> String {
        let base = base_url.trim_end_matches('/');
        match self {
            Surface::OpenAi => format!("{base}/chat/completions"),
            Surface::Anthropic => format!("{base}/messages"),
        }
    }
}

/// Attach the tenant's optional provider headers (org + custom) to an upstream
/// request. An empty metadata is a no-op.
pub(super) fn apply_provider_headers(
    mut request: reqwest::RequestBuilder,
    metadata: &providers::ProviderMetadata,
) -> reqwest::RequestBuilder {
    if let Some(org) = &metadata.org {
        request = request.header("OpenAI-Organization", org);
    }
    for (header, value) in &metadata.headers {
        request = request.header(header, value);
    }
    request
}

/// An upstream request with the provider's auth and metadata headers. A keyless
/// local provider sends no Authorization header at all (ITEM-082); every other
/// kind sends its credential (Anthropic on `x-api-key`).
pub(crate) fn provider_request(client: &reqwest::Client, route: &Route) -> reqwest::RequestBuilder {
    let mut request = client
        .post(&route.url)
        .header("Content-Type", "application/json");
    for (header, value) in providers::auth_headers(&route.kind, &route.key, &route.metadata) {
        request = request.header(header, value);
    }
    apply_provider_headers(request, &route.metadata)
}

/// The adaptive policy's health weight for a provider (1.0 = no opinion).
pub(super) fn weight_of(weights: &std::collections::BTreeMap<String, f64>, id: &Uuid) -> f64 {
    weights.get(&id.to_string()).copied().unwrap_or(1.0)
}

pub(crate) async fn resolve_route(
    state: &Arc<AppState>,
    tenant: &Tenant,
    model: Option<&str>,
    surface: Surface,
    weights: Option<&std::collections::BTreeMap<String, f64>>,
) -> Result<Route, Response> {
    // ITEM-100: when several healthy providers serve the model, spread the
    // load across them, weighted by the adaptive policy's health weights
    // (ITEM-069). A single-provider model is the old behaviour exactly.
    if let Some(model) = model {
        if let Some(route) = balance_across_providers(state, tenant, model, surface, weights).await
        {
            return route;
        }
    }
    let resolved = state.provider_store.resolve_for(tenant.id, model).await;
    match resolved {
        Ok(Some(provider)) => {
            // ITEM-094: a provider that speaks the other surface is reachable
            // through the translation seam when the tenant allows it; without
            // it the typed error of ITEM-050 AC5 stands.
            let native = if providers::is_anthropic(&provider.kind) {
                Surface::Anthropic
            } else {
                Surface::OpenAi
            };
            let translate = (native != surface).then_some(native);
            if translate.is_some() && !state.hotpath.translation_enabled(state, tenant.id).await {
                return Err(surface_mismatch(surface));
            }
            Ok(Route {
                url: native.endpoint(&provider.base_url),
                is_local: providers::is_local(&provider.kind),
                key: provider.api_key,
                metadata: provider.metadata,
                provider_id: Some(provider.id),
                kind: provider.kind,
                translate,
                selection: Some("only"),
            })
        }
        Ok(None) => {
            if surface == Surface::Anthropic {
                return Err(surface_mismatch(surface));
            }
            match tenant.upstream_openai_key.as_deref() {
                Some(key) => Ok(Route {
                    url: state.upstream_url.clone(),
                    key: key.to_string(),
                    metadata: providers::ProviderMetadata::default(),
                    provider_id: None,
                    kind: "openai".to_string(),
                    is_local: false,
                    translate: (surface == Surface::Anthropic).then_some(Surface::OpenAi),
                    selection: Some("only"),
                }),
                None => Err(json_error(
                    StatusCode::BAD_REQUEST,
                    "no_provider",
                    "no upstream provider configured; add one in Dashboard → Providers",
                )),
            }
        }
        Err(e) => {
            tracing::error!("provider resolution failed: {e}");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "provider_error",
                "could not resolve your provider",
            ))
        }
    }
}

/// ITEM-100: distribute across the providers that serve `model`.
///
/// Returns `None` when there is nothing to balance (one provider, or none), so
/// the caller keeps the single-provider path byte-for-byte. Unreachable
/// providers (a failed reachability probe) and policy-zeroed ones are skipped;
/// the seed is a per-(tenant, model) counter, so consecutive requests spread.
pub(super) async fn balance_across_providers(
    state: &Arc<AppState>,
    tenant: &Tenant,
    model: &str,
    surface: Surface,
    weights: Option<&std::collections::BTreeMap<String, f64>>,
) -> Option<Result<Route, Response>> {
    // One lookup for the whole candidate set (the hot path cannot afford a
    // query per provider). Unreachable providers are not candidates: the
    // reachability probe already told us, and an attempt would only fail.
    let resolved = state
        .provider_store
        .resolved_for_model(tenant.id, model)
        .await
        .ok()?;
    let eligible: Vec<_> = resolved
        .into_iter()
        .filter(|provider| provider.last_check_ok != Some(false))
        .collect();
    if eligible.len() < 2 {
        return None;
    }
    let empty = std::collections::BTreeMap::new();
    let weights = weights.unwrap_or(&empty);
    let candidates: Vec<crate::loadbalance::Candidate> = eligible
        .iter()
        .enumerate()
        .map(|(slot, provider)| crate::loadbalance::Candidate {
            slot,
            weight: weight_of(weights, &provider.id),
        })
        .collect();
    let seed = state.hotpath.next_seed(tenant.id, model);
    let choice =
        crate::loadbalance::choose(&candidates, seed, crate::loadbalance::Strategy::Weighted)?;
    let provider = &eligible[choice.slot];
    let native = if providers::is_anthropic(&provider.kind) {
        Surface::Anthropic
    } else {
        Surface::OpenAi
    };
    let translate = (native != surface).then_some(native);
    if translate.is_some() && !state.hotpath.translation_enabled(state, tenant.id).await {
        return Some(Err(surface_mismatch(surface)));
    }
    Some(Ok(Route {
        url: native.endpoint(&provider.base_url),
        is_local: providers::is_local(&provider.kind),
        key: provider.api_key.clone(),
        metadata: provider.metadata.clone(),
        provider_id: Some(provider.id),
        kind: provider.kind.clone(),
        translate,
        selection: Some(choice.reason.as_str()),
    }))
}

/// A clear typed error when the resolved provider's kind does not match the
/// surface the request arrived on (ITEM-050 AC5).
pub(super) fn surface_mismatch(surface: Surface) -> Response {
    let message = match surface {
        Surface::Anthropic => "no Anthropic provider configured; add one in Dashboard → Providers",
        Surface::OpenAi => {
            "no OpenAI-compatible provider configured; add one in Dashboard → Providers"
        }
    };
    json_error(StatusCode::BAD_REQUEST, "no_provider", message)
}

/// Read and parse a JSON request body, or the typed error response.
/// Returns the raw bytes too — the Anthropic surface forwards them unchanged.
pub(super) async fn read_json_body(req: Request) -> Result<(axum::body::Bytes, Value), Response> {
    let body_bytes = axum::body::to_bytes(req.into_body(), MAX_REQUEST_BYTES)
        .await
        .map_err(|_| {
            json_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "request body exceeds 10MB",
            )
        })?;
    let payload = serde_json::from_slice(&body_bytes).map_err(|_| {
        json_error(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            "request body must be valid JSON",
        )
    })?;
    Ok((body_bytes, payload))
}

/// One attempt in the chain: the model, its resolved route, and the failures
/// that may send the request here (ITEM-109).
#[derive(Clone)]
pub(crate) struct ChainStep {
    pub(crate) model: String,
    pub(crate) route: Route,
    pub(crate) triggers: Vec<routing::FallbackTrigger>,
}

/// The ordered, de-duplicated chain to attempt: the primary (after any
/// router/budget override), then the tenant's fallback steps, dropping models
/// the tenant disabled (ITEM-044) and steps no provider can serve (ITEM-132).
/// Each entry carries the provider that owns it, so a fallback to another
/// provider's model hits the right upstream. Always contains at least the
/// primary.
pub(super) async fn build_model_chain(
    state: &Arc<AppState>,
    tenant: &Tenant,
    route: &Route,
    primary: String,
    fallbacks: Vec<routing::FallbackStep>,
    weights: Option<&std::collections::BTreeMap<String, f64>>,
) -> Vec<ChainStep> {
    let mut seen = std::collections::HashSet::new();
    let mut chain = Vec::new();
    let entry = routing::FallbackStep::retryable(primary.clone());
    for step in std::iter::once(entry).chain(fallbacks) {
        if !seen.insert(step.model.clone()) {
            continue;
        }
        let decision = if step.model == primary {
            // The primary was already checked by `ensure_model_enabled`.
            crate::hotpath::Decision {
                route: route.clone(),
                disabled: false,
                serves: true,
            }
        } else {
            // ITEM-132: a step owns its provider. Neither an unresolvable step
            // nor one no provider serves may borrow the primary's route — that
            // dispatches a model to an upstream that never offered it, and the
            // caller reads the provider's own confusing 4xx.
            match state
                .hotpath
                .route(state, tenant, &step.model, Surface::OpenAi, weights)
                .await
            {
                Ok(decision) => decision,
                Err(_) => {
                    tracing::warn!(
                        "fallback step {} dropped: its route could not be resolved",
                        step.model
                    );
                    continue;
                }
            }
        };
        if decision.disabled {
            continue;
        }
        if !decision.serves {
            tracing::warn!(
                "fallback step {} dropped: no provider for this workspace advertises it",
                step.model
            );
            continue;
        }
        chain.push(ChainStep {
            model: step.model,
            route: decision.route,
            triggers: step.triggers,
        });
    }
    if chain.is_empty() {
        chain.push(ChainStep {
            model: primary,
            route: route.clone(),
            triggers: routing::FallbackTrigger::RETRYABLE.to_vec(),
        });
    }
    chain
}

/// What the router decided for a request: the route for the client's model and
/// the ordered chain to try. Fills in the trace-visible decisions on `meta`
/// (provider, router event, cache affinity) as it goes.
pub(super) struct ModelPlan {
    pub(super) route: Route,
    /// The model the chain starts from (after any override/affinity) — recorded
    /// as the `from_model` of a fallback event.
    pub(super) primary: String,
    pub(super) chain: Vec<ChainStep>,
    /// ITEM-091: the tenant's pruning policy, read with the rest of the policy.
    pub(super) prune: Option<crate::prune::PrunePolicy>,
}

/// Turn the request into a model plan: read the adaptive policy, resolve the
/// route, apply the budget breaker and the heuristic router, prefer a warm
/// cache, then append the tenant's fallback chain — dropping models the tenant
/// disabled. Trace-visible decisions (provider, router event, cache affinity)
/// are recorded on `meta` as they are made.
pub(super) async fn plan_models(
    state: &Arc<AppState>,
    tenant: &Tenant,
    meta: &mut RequestMeta,
    started: Instant,
    stages: &mut crate::stages::Stages,
) -> Result<ModelPlan, Response> {
    let tenant_id = tenant.id;
    // ITEM-064/069: read the adaptive policy once — it supplies both the router
    // threshold and the provider health weights.
    let policy = state.hotpath.policy(state, tenant_id).await;
    stages.mark("policy");
    meta.policy_version = policy.as_ref().map(|policy| policy.version);
    let policy_weights = policy.as_ref().map(|policy| &policy.provider_weights);
    let decision = match state
        .hotpath
        .route(state, tenant, &meta.model, Surface::OpenAi, policy_weights)
        .await
    {
        Ok(decision) => decision,
        // ITEM-123: a refusal is evidence. `no_provider` is the most common first
        // failure, so it is traced and carries the id of the row it wrote.
        Err(resp) => {
            trace_error(state, meta, tenant_id, started, 400, ErrorClass::Inferon4xx);
            return Err(super::response::with_trace_id(resp, meta.id));
        }
    };
    let route = decision.route;
    meta.provider_id = route.provider_id;
    meta.is_local = route.is_local;
    meta.provider_selection = route.selection;
    stages.mark("route");
    crate::metrics::request_routed(route.provider_id);

    // ITEM-044 AC5: a model the tenant disabled is refused with a clear typed
    // error (and traced), not a raw upstream 4xx. ITEM-123: the id goes on it.
    if let Err(resp) = ensure_model_enabled(state, meta, tenant_id, started, decision.disabled) {
        return Err(super::response::with_trace_id(resp, meta.id));
    }

    // ITEM-016: the budget breaker wins over the router and rejects before any
    // upstream call; ITEM-046/081: otherwise the router may downshift.
    let policy_threshold = policy.as_ref().and_then(|policy| policy.router_threshold);
    // ITEM-140: the projection the budget breaker was measuring against, priced at
    // the model the caller asked for. A refusal is the one case where "prevented"
    // is the whole request.
    let projected = crate::cost::projected_cost(
        &meta.model,
        crate::ratelimit::estimate_tokens(meta.payload_bytes()),
        crate::cost::requested_max_tokens(&meta.raw_prompt),
    );
    let chosen =
        match budget::check_budget(state, tenant_id, &meta.raw_prompt, policy_threshold).await {
            Ok(chosen) => chosen,
            Err(resp) => {
                meta.prevented = Some((projected, "budget"));
                trace_error(state, meta, tenant_id, started, 429, ErrorClass::Inferon4xx);
                return Err(super::response::with_trace_id(resp, meta.id));
            }
        };
    // The 85% zone served this on the cheap tier: the protection saved the delta
    // between what was asked for and what was used.
    if let Some(chosen) = &chosen {
        if chosen.reason == budget::BUDGET_REASON {
            let served = crate::cost::projected_cost(
                &chosen.model,
                crate::ratelimit::estimate_tokens(meta.payload_bytes()),
                crate::cost::requested_max_tokens(&meta.raw_prompt),
            );
            let avoided = projected - served;
            if avoided > rust_decimal::Decimal::ZERO {
                meta.prevented = Some((avoided, "downgrade"));
            }
        }
    }
    stages.mark("budget");
    let primary = override_primary(state, route.provider_id, meta, chosen).await;
    // ITEM-063: prefer a warm provider cache over a marginally cheaper miss.
    let primary = prefer_warm(state, tenant_id, meta, primary).await;
    let fallbacks = state.hotpath.fallbacks(state, tenant_id).await;
    let chain = build_model_chain(
        state,
        tenant,
        &route,
        primary.clone(),
        fallbacks,
        policy_weights,
    )
    .await;
    stages.mark("chain");
    Ok(ModelPlan {
        route,
        primary,
        chain,
        prune: policy.as_ref().and_then(|policy| policy.prune.clone()),
    })
}

/// ITEM-044 AC4: never select a model the tenant disabled — the requested model
/// then stands. Otherwise the override becomes the primary and, when the
/// heuristic router caused it, is recorded on the trace (ITEM-054/081).
pub(super) async fn override_primary(
    state: &Arc<AppState>,
    provider_id: Option<Uuid>,
    meta: &mut RequestMeta,
    chosen: Option<budget::ModelOverride>,
) -> String {
    let Some(chosen) = chosen else {
        return meta.model.clone();
    };
    if state
        .model_store
        .is_disabled(provider_id, &chosen.model)
        .await
        .unwrap_or(false)
    {
        return meta.model.clone();
    }
    if chosen.reason == budget::ROUTER_REASON {
        let mut event = json!({
            "from_model": meta.model,
            "to_model": chosen.model,
            "reason": chosen.reason,
        });
        if let Some(source) = chosen.pair_source {
            event["pair_source"] = json!(source.label());
        }
        meta.router_event = Some(event);
    }
    chosen.model
}

/// ITEM-063: keep a warm provider cache over a marginally cheaper miss.
pub(super) async fn prefer_warm(
    state: &Arc<AppState>,
    tenant_id: Uuid,
    meta: &mut RequestMeta,
    primary: String,
) -> String {
    let Some(hash) = meta.system_prompt_hash.as_deref() else {
        return primary;
    };
    match state.affinity_store.warm_target(tenant_id, hash).await {
        Ok(Some(warm))
            if warm.model != primary && crate::affinity::keep_warm(&warm.model, &primary) =>
        {
            meta.cache_affinity = Some(json!({
                "from_model": primary,
                "to_model": warm.model,
                "reason": "cache_affinity",
            }));
            warm.model
        }
        _ => primary,
    }
}
