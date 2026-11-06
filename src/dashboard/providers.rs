//! The provider and model control surface.

use super::*;
/// The upstream base URL for display: the configured `UPSTREAM_URL` without the
/// `/chat/completions` suffix.
pub(crate) fn upstream_base_url(url: &str) -> String {
    url.trim_end_matches('/')
        .trim_end_matches("/chat/completions")
        .to_string()
}

fn provider_json(provider: &crate::providers::Provider) -> Value {
    json!({
        "id": provider.id.to_string(),
        "kind": provider.kind,
        "name": provider.name,
        "base_url": provider.base_url,
        "key_hint": provider.key_hint,
        "metadata": provider.metadata,
        "spend_limit_cents": provider.spend_limit_cents,
        "is_default": provider.is_default,
        "last_check_at": provider.last_check_at,
        "last_check_ok": provider.last_check_ok,
        "created_at": provider.created_at,
        "updated_at": provider.updated_at,
    })
}

/// One advertised model, merged with the provider's selection flags and the
/// cost engine's price (unknown ids report `priced: false`).
fn model_json(model_id: &str, settings: &[crate::models::ModelSetting], local: bool) -> Value {
    let setting = settings.iter().find(|setting| setting.model_id == model_id);
    // A model served by a local provider is free at the margin (ITEM-077) — show
    // that, not a cloud price keyed off the model's name (ITEM-082).
    let price = crate::cost::price_for(model_id);
    let (input, output) = if local {
        ("0".to_string(), "0".to_string())
    } else {
        (price.prompt_miss.to_string(), price.completion.to_string())
    };
    json!({
        "id": model_id,
        "enabled": setting.map(|setting| setting.enabled).unwrap_or(true),
        "is_default": setting.map(|setting| setting.is_default).unwrap_or(false),
        "price": {
            "input_usd_per_million": input,
            "output_usd_per_million": output,
            "priced": local || crate::cost::is_priced(model_id),
            "local": local,
        },
    })
}

/// Read a provider's advertised models from `GET <base>/models`.
pub(crate) async fn fetch_provider_models(
    state: &Arc<AppState>,
    provider: &crate::providers::ResolvedProvider,
) -> Result<Vec<String>, String> {
    crate::providers::fetch_models(&state.http_client, provider).await
}

/// GET /v1/dashboard/api/providers (session-scoped) — masked provider list.
pub async fn tenant_providers(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    match state.provider_store.list(tenant_id).await {
        Ok(providers) => json_ok(json!({
            "providers": providers.iter().map(provider_json).collect::<Vec<_>>(),
            "kinds": crate::providers::PROVIDER_KINDS,
            "role": role,
        })),
        Err(e) => internal(e.to_string()),
    }
}

/// Trim, drop a trailing slash and require http(s) — shared by the provider
/// create and update paths.
pub(crate) fn normalize_base_url(raw: &str) -> Result<String, &'static str> {
    let url = raw.trim().trim_end_matches('/').to_string();
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(url)
    } else {
        Err("base URL must start with http:// or https://")
    }
}

/// POST /v1/dashboard/api/providers (owner-only) — register a provider.
pub async fn tenant_provider_create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ProviderBody>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    if !crate::providers::is_supported_kind(&body.kind) {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "unsupported_kind",
            "that provider kind is not supported yet",
        );
    }
    let base_url = match normalize_base_url(&body.base_url) {
        Ok(url) => url,
        Err(message) => return crate::json_error(StatusCode::BAD_REQUEST, "invalid_base_url", message),
    };
    let key = body.key.trim().to_string();
    if key.is_empty() && crate::providers::requires_key(&body.kind) {
        return crate::json_error(StatusCode::BAD_REQUEST, "invalid_key", "a provider key is required");
    }
    let name = body
        .name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| crate::providers::default_name(&body.kind));
    let new = crate::providers::NewProvider {
        kind: body.kind,
        name,
        base_url,
        key,
        metadata: body.metadata,
        spend_limit_cents: body.spend_limit_cents.unwrap_or(0),
    };
    match state.provider_store.insert(tenant_id, &new).await {
        Ok(id) => {
            // ITEM-092: a new provider can change how every model routes.
            state.hotpath.invalidate(tenant_id);
            json_ok(json!({ "id": id.to_string() }))
        }
        Err(e) => {
            tracing::error!("provider insert failed: {e}");
            internal(e.to_string())
        }
    }
}

/// POST /v1/dashboard/api/providers/:provider_id/remove (owner-only).
pub async fn tenant_provider_remove(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(provider_id): Path<uuid::Uuid>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    match state.provider_store.remove(tenant_id, provider_id).await {
        Ok(removed) => {
            state.hotpath.invalidate(tenant_id);
            json_ok(json!({
                "provider_id": provider_id.to_string(),
                "removed": removed,
            }))
        }
        Err(e) => internal(e.to_string()),
    }
}

/// POST /v1/dashboard/api/providers/:provider_id (owner-only) — rotate/update a
/// provider in place (ITEM-043 AC5). Omitted fields are left unchanged.
pub async fn tenant_provider_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(provider_id): Path<uuid::Uuid>,
    Json(body): Json<ProviderUpdateBody>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    let base_url = match body.base_url {
        Some(url) => match normalize_base_url(&url) {
            Ok(url) => Some(url),
            Err(message) => {
                return crate::json_error(StatusCode::BAD_REQUEST, "invalid_base_url", message)
            }
        },
        None => None,
    };
    let key = body
        .key
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty());
    let update = crate::providers::ProviderUpdate {
        name: body
            .name
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty()),
        base_url,
        key,
        metadata: body.metadata,
        spend_limit_cents: body.spend_limit_cents,
    };
    match state.provider_store.update(tenant_id, provider_id, &update).await {
        Ok(true) => {
            state.hotpath.invalidate(tenant_id);
            json_ok(json!({ "provider_id": provider_id.to_string(), "updated": true }))
        }
        Ok(false) => {
            crate::json_error(StatusCode::NOT_FOUND, "not_found", "no such provider")
        }
        Err(e) => internal(e.to_string()),
    }
}

/// POST /v1/dashboard/api/providers/:provider_id/default (owner-only) — make a
/// provider the tenant's default (the fallback for models no kind owns).
pub async fn tenant_provider_default(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(provider_id): Path<uuid::Uuid>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    match state.provider_store.set_default(tenant_id, provider_id).await {
        Ok(true) => {
            state.hotpath.invalidate(tenant_id);
            json_ok(json!({
                "provider_id": provider_id.to_string(),
                "is_default": true,
            }))
        }
        Ok(false) => crate::json_error(StatusCode::NOT_FOUND, "not_found", "no such provider"),
        Err(e) => internal(e.to_string()),
    }
}

/// POST /v1/dashboard/api/providers/test (owner-only) — probe an **unsaved**
/// provider config (`GET <base>/models`, the same call Test-connection makes)
/// so a connection can be verified before it is added. Nothing is persisted;
/// the key is used for the probe only.
pub async fn tenant_provider_probe(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ProviderProbeBody>,
) -> Response {
    let (_tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    let base_url = match normalize_base_url(&body.base_url) {
        Ok(url) => url,
        Err(message) => return crate::json_error(StatusCode::BAD_REQUEST, "invalid_base_url", message),
    };
    let key = body.key.trim().to_string();
    let kind = body.kind.unwrap_or_default();
    if key.is_empty() && crate::providers::requires_key(&kind) {
        return crate::json_error(StatusCode::BAD_REQUEST, "invalid_key", "a provider key is required");
    }
    let resolved = crate::providers::ResolvedProvider {
            last_check_ok: None,
        id: uuid::Uuid::nil(),
        kind,
        base_url,
        api_key: key,
        metadata: body.metadata,
    };
    match fetch_provider_models(&state, &resolved).await {
        Ok(models) => json_ok(json!({ "ok": true, "models": models })),
        Err(e) => {
            tracing::warn!("provider probe failed: {e}");
            json_ok(json!({ "ok": false, "models": [], "error": e }))
        }
    }
}

/// POST /v1/dashboard/api/providers/:provider_id/test — a minimal probe of the
/// provider's base URL + key (`GET <base>/models`, the OpenAI-compatible
/// endpoint; costs nothing). Reports `{ ok, status }` — `status` is `null` when
/// the provider could not be reached.
pub async fn tenant_provider_test(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(provider_id): Path<uuid::Uuid>,
) -> Response {
    let (tenant_id, _role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    let provider = match state.provider_store.resolve_provider(tenant_id, provider_id).await {
        Ok(Some(provider)) => provider,
        Ok(None) => {
            return crate::json_error(StatusCode::NOT_FOUND, "not_found", "no such provider")
        }
        Err(e) => return internal(e.to_string()),
    };
    let url = format!("{}/models", provider.base_url.trim_end_matches('/'));
    let mut probe = state
        .http_client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10));
    for (header, value) in crate::providers::auth_headers(&provider.kind, &provider.api_key, &provider.metadata) {
        probe = probe.header(header, value);
    }
    if let Some(org) = &provider.metadata.org {
        probe = probe.header("OpenAI-Organization", org);
    }
    for (header, value) in &provider.metadata.headers {
        probe = probe.header(header, value);
    }
    let probe = probe.send().await;
    match probe {
        Ok(response) => {
            let status = response.status();
            // A successful probe is also a model sync: cache what the host
            // advertises so the dashboard and the proxy can route by model.
            let mut models = 0usize;
            if status.is_success() {
                if let Ok(body) = response.json::<Value>().await {
                    let ids = crate::providers::parse_model_ids(&body);
                    models = ids.len();
                    if let Err(e) = state.provider_store.record_models(provider_id, &ids).await {
                        tracing::warn!("provider model sync failed for {provider_id}: {e}");
                    } else {
                        state.hotpath.invalidate(tenant_id);
                    }
                }
            }
            let _ = state
                .provider_store
                .record_check(tenant_id, provider_id, status.is_success())
                .await;
            json_ok(json!({
                "ok": status.is_success(),
                "status": status.as_u16(),
                "models": models,
                // Diagnosis for a failure: the upstream's HTTP status.
                "error": (!status.is_success()).then(|| format!("upstream returned {status}")),
            }))
        }
        Err(e) => {
            tracing::warn!("provider probe failed for {provider_id}: {e}");
            let _ = state
                .provider_store
                .record_check(tenant_id, provider_id, false)
                .await;
            json_ok(json!({
                "ok": false,
                "status": null,
                "models": 0,
                "error": format!("unreachable: {e}"),
            }))
        }
    }
}

/// GET /v1/dashboard/api/providers/:provider_id/models — discover the provider's
/// current model list from `GET <base>/models` (live), cache it, and return it
/// with this provider's selection flags. Never a static catalog.
pub async fn tenant_provider_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(provider_id): Path<uuid::Uuid>,
) -> Response {
    let (tenant_id, _role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    let provider = match state.provider_store.resolve_provider(tenant_id, provider_id).await {
        Ok(Some(provider)) => provider,
        Ok(None) => {
            return crate::json_error(StatusCode::NOT_FOUND, "not_found", "no such provider")
        }
        Err(e) => return internal(e.to_string()),
    };
    let ids = match fetch_provider_models(&state, &provider).await {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!("provider model discovery failed for {provider_id}: {e}");
            return crate::json_error(
                StatusCode::BAD_GATEWAY,
                "provider_models_failed",
                "could not read the model list from the provider",
            );
        }
    };
    if let Err(e) = state.provider_store.record_models(provider_id, &ids).await {
        return internal(e.to_string());
    }
    let settings = match state.model_store.settings(provider_id).await {
        Ok(settings) => settings,
        Err(e) => return internal(e.to_string()),
    };
    let local = crate::providers::is_local(&provider.kind);
    let models: Vec<Value> = ids.iter().map(|id| model_json(id, &settings, local)).collect();
    state.hotpath.invalidate(tenant_id);
    json_ok(json!({ "models": models }))
}

/// GET /v1/dashboard/api/models — every provider's cached advertised models
/// (no live call), grouped by provider, with selection flags + prices.
pub async fn tenant_models(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    let providers = match state.provider_store.list(tenant_id).await {
        Ok(providers) => providers,
        Err(e) => return internal(e.to_string()),
    };
    let mut out = Vec::with_capacity(providers.len());
    for provider in &providers {
        let ids = match state.provider_store.advertised_models(provider.id).await {
            Ok(ids) => ids,
            Err(e) => return internal(e.to_string()),
        };
        let settings = match state.model_store.settings(provider.id).await {
            Ok(settings) => settings,
            Err(e) => return internal(e.to_string()),
        };
        let local = crate::providers::is_local(&provider.kind);
        let models: Vec<Value> = ids.iter().map(|id| model_json(id, &settings, local)).collect();
        out.push(json!({
            "id": provider.id.to_string(),
            "name": provider.name,
            "kind": provider.kind,
            "is_default": provider.is_default,
            "models": models,
        }));
    }
    json_ok(json!({ "providers": out, "role": role }))
}

/// POST /v1/dashboard/api/providers/:provider_id/models/:model_id (owner-only)
/// — enable/disable a model the provider advertises and/or make it that
/// provider's default.
pub async fn tenant_provider_model_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((provider_id, model_id)): Path<(uuid::Uuid, String)>,
    Json(body): Json<ModelUpdateBody>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    // Ownership check (the id must be one of this tenant's providers).
    let owned = match state.provider_store.list(tenant_id).await {
        Ok(providers) => providers.into_iter().any(|provider| provider.id == provider_id),
        Err(e) => return internal(e.to_string()),
    };
    if !owned {
        return crate::json_error(StatusCode::NOT_FOUND, "not_found", "no such provider");
    }
    // Only a model the provider actually advertises can be configured.
    let advertised = match state.provider_store.advertised_models(provider_id).await {
        Ok(ids) => ids,
        Err(e) => return internal(e.to_string()),
    };
    if !advertised.iter().any(|id| id == &model_id) {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "unknown_model",
            "the provider does not advertise that model",
        );
    }
    if let Some(enabled) = body.enabled {
        if let Err(e) = state.model_store.set_enabled(provider_id, &model_id, enabled).await {
            return internal(e.to_string());
        }
    }
    if body.is_default == Some(true) {
        if let Err(e) = state.model_store.set_default(provider_id, &model_id).await {
            return internal(e.to_string());
        }
    }
    state.hotpath.invalidate(tenant_id);
    json_ok(json!({ "provider_id": provider_id.to_string(), "model_id": model_id }))
}

/// True when the provider that owns `model` has explicitly disabled it — the
/// routing chain's up-front check.
pub(crate) async fn model_is_disabled(state: &Arc<AppState>, tenant_id: uuid::Uuid, model: &str) -> bool {
    let provider_id = state
        .provider_store
        .resolve_for(tenant_id, Some(model))
        .await
        .ok()
        .flatten()
        .map(|provider| provider.id);
    state
        .model_store
        .is_disabled(provider_id, model)
        .await
        .unwrap_or(false)
}

#[derive(serde::Deserialize)]
pub struct ProviderBody {
    kind: String,
    name: Option<String>,
    base_url: String,
    /// ITEM-129: absent is the same as empty. A `local` provider needs no
    /// credential at all, and a keyed kind refuses an empty one with a typed
    /// error rather than a deserialisation message.
    #[serde(default)]
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

#[derive(serde::Deserialize)]
pub struct ModelUpdateBody {
    enabled: Option<bool>,
    is_default: Option<bool>,
}

