//! The operator surface: platform-wide, cross-tenant aggregates and the
//! governance actions. Split from `mod.rs` so the tenant surface and the
//! operator surface do not share a file.

use super::*;

/// Operator-only handlers: need the operator capability. A tenant credential
/// without it is rejected (403).
pub(crate) async fn operator_or(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<(), Response> {
    if resolve_scope(state, headers).await?.is_operator {
        Ok(())
    } else {
        Err(forbidden())
    }
}

/// GET /v1/dashboard/api/operator/tenants?days=7 — per-tenant spend + volume.
pub async fn operator_tenants(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    let rows: Vec<(String, i64, i64, Decimal)> = match sqlx::query_as(
        "SELECT tenant_id::text, COUNT(*), COUNT(*) FILTER (WHERE status_code >= 400),
                COALESCE(SUM(upstream_cost_usd), 0)
         FROM traces
         WHERE created_at >= NOW() - ($1 * INTERVAL '1 day')
         GROUP BY tenant_id ORDER BY 4 DESC",
    )
    .bind(window.days())
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) => rows,
        Err(e) => return db_err(e),
    };
    let tenants: Vec<Value> = rows
        .into_iter()
        .map(|(tenant_id, requests, errors, cost)| {
            json!({
                "tenant_id": tenant_id,
                "requests": requests,
                "errors": errors,
                "cost_usd": cost.to_string(),
            })
        })
        .collect();
    json_ok(json!({"tenants": tenants, "window_days": window.days()}))
}

/// GET /v1/dashboard/api/operator/metrics — telemetry pipeline health (ITEM-059).
pub async fn operator_metrics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    let mut value = crate::metrics::snapshot();
    // ITEM-060: telemetry queue depth, read from the live channel.
    let capacity = state.telemetry_tx.max_capacity();
    if let Some(object) = value.as_object_mut() {
        let depth = capacity.saturating_sub(state.telemetry_tx.capacity());
        object.insert("telemetry_queue_depth".to_string(), json!(depth));
        object.insert("telemetry_queue_capacity".to_string(), json!(capacity));
    }
    json_ok(value)
}

/// Relative change from `previous` to `current` (`None` when previous is 0).
pub(crate) fn wow(current: i64, previous: i64) -> Option<f64> {
    (previous > 0).then(|| (current - previous) as f64 / previous as f64)
}

/// GET /v1/dashboard/api/operator/activation?days=7 — engagement, not billing:
/// active tenants/keys, requests per active developer, the first-request funnel,
/// and the week-over-week trend. Operator-only, tenant-anonymised aggregates.
pub async fn operator_activation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    let days = window.days();
    // Activity this window vs the preceding window of the same length.
    let activity: Option<(i64, i64, i64, i64, i64, i64)> = match sqlx::query_as(
        "SELECT
            COUNT(DISTINCT tenant_id) FILTER (WHERE created_at >= NOW() - $1 * INTERVAL '1 day'),
            COUNT(DISTINCT api_key_id) FILTER (WHERE created_at >= NOW() - $1 * INTERVAL '1 day'),
            COUNT(*) FILTER (WHERE created_at >= NOW() - $1 * INTERVAL '1 day'),
            COUNT(DISTINCT tenant_id) FILTER (WHERE created_at >= NOW() - ($1 * 2) * INTERVAL '1 day'
                                                AND created_at < NOW() - $1 * INTERVAL '1 day'),
            COUNT(DISTINCT api_key_id) FILTER (WHERE created_at >= NOW() - ($1 * 2) * INTERVAL '1 day'
                                                AND created_at < NOW() - $1 * INTERVAL '1 day'),
            COUNT(*) FILTER (WHERE created_at >= NOW() - ($1 * 2) * INTERVAL '1 day'
                               AND created_at < NOW() - $1 * INTERVAL '1 day')
         FROM traces
         WHERE created_at >= NOW() - ($1 * 2) * INTERVAL '1 day'",
    )
    .bind(days)
    .fetch_optional(&state.db)
    .await
    {
        Ok(row) => row,
        Err(e) => return db_err(e),
    };
    let (active_tenants, active_keys, requests, prev_tenants, prev_keys, prev_requests) =
        activity.unwrap_or((0, 0, 0, 0, 0, 0));

    // New tenant accounts (signups) this window vs the previous one.
    let signups: (i64, i64) = match sqlx::query_as(
        "SELECT COUNT(*) FILTER (WHERE created_at >= NOW() - $1 * INTERVAL '1 day'),
                COUNT(*) FILTER (WHERE created_at >= NOW() - ($1 * 2) * INTERVAL '1 day'
                                   AND created_at < NOW() - $1 * INTERVAL '1 day')
         FROM tenants",
    )
    .bind(days)
    .fetch_one(&state.db)
    .await
    {
        Ok(row) => row,
        Err(e) => return db_err(e),
    };

    // First-request funnel over the window's signup cohort.
    let funnel: (i64, i64, i64, i64) = match sqlx::query_as(
        "WITH cohort AS (SELECT id FROM tenants WHERE created_at >= NOW() - $1 * INTERVAL '1 day')
         SELECT
           (SELECT COUNT(*) FROM cohort),
           (SELECT COUNT(*) FROM cohort c
              WHERE EXISTS (SELECT 1 FROM providers p WHERE p.tenant_id = c.id)),
           (SELECT COUNT(*) FROM cohort c
              WHERE EXISTS (SELECT 1 FROM traces t WHERE t.tenant_id = c.id)),
           (SELECT COUNT(*) FROM cohort c
              WHERE (SELECT COUNT(DISTINCT date_trunc('day', t.created_at))
                       FROM traces t WHERE t.tenant_id = c.id) >= 2)",
    )
    .bind(days)
    .fetch_one(&state.db)
    .await
    {
        Ok(row) => row,
        Err(e) => return db_err(e),
    };

    let per_tenant = rate(requests, active_tenants);
    let per_key = rate(requests, active_keys);
    json_ok(json!({
        "window_days": days,
        "active_tenants": active_tenants,
        "active_keys": active_keys,
        "requests": requests,
        "requests_per_active_tenant": per_tenant,
        "requests_per_active_key": per_key,
        "signups": signups.0,
        "previous_signups": signups.1,
        "previous_active_tenants": prev_tenants,
        "previous_active_keys": prev_keys,
        "previous_requests": prev_requests,
        // Week-over-week trend (fraction; positive = growth).
        "active_tenants_wow": wow(active_tenants, prev_tenants),
        "requests_wow": wow(requests, prev_requests),
        "signups_wow": wow(signups.0, signups.1),
        "funnel": {
            "signed_up": funnel.0,
            "created_provider": funnel.1,
            "first_request": funnel.2,
            "returned": funnel.3,
        },
    }))
}

/// GET /v1/dashboard/api/operator/policy/:tenant_id — the tenant's active
/// adaptive policy (version + evidence), operator-only (ITEM-064).
pub async fn operator_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(tenant_id): Path<uuid::Uuid>,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    match state.policy_store.active(tenant_id).await {
        Ok(policy) => json_ok(json!({
            "tenant_id": tenant_id.to_string(),
            "policy": policy,
        })),
        Err(e) => internal(&e.to_string()),
    }
}

/// POST /v1/dashboard/api/operator/policy/:tenant_id/freeze — the kill switch:
/// disable adaptation so the static config applies again.
pub async fn operator_policy_freeze(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(tenant_id): Path<uuid::Uuid>,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    match state.policy_store.freeze(tenant_id).await {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(e) => internal(&e.to_string()),
    }
}

/// POST /v1/dashboard/api/operator/policy/:tenant_id/rollback/:version —
/// re-activate a previous version (a new version is appended, auditable).
pub async fn operator_policy_rollback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, version)): Path<(uuid::Uuid, i64)>,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    match state.policy_store.rollback(tenant_id, version).await {
        Ok(Some(policy)) => json_ok(json!({ "ok": true, "policy": policy })),
        Ok(None) => crate::json_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "no such policy version",
        ),
        Err(e) => internal(&e.to_string()),
    }
}

/// GET /v1/dashboard/api/operator/summary?days=7 — platform-wide totals.
pub async fn operator_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    match operator_or(&state, &headers).await {
        Ok(()) => query_summary(&state, None, &window, "operator").await,
        Err(resp) => resp,
    }
}

/// GET /v1/dashboard/api/operator/by-model?days=7 — platform-wide spend by model.
pub async fn operator_by_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    match operator_or(&state, &headers).await {
        Ok(()) => query_by_model(&state, None, &window).await,
        Err(resp) => resp,
    }
}

/// GET /v1/dashboard/api/operator/by-prompt?days=7 — platform-wide clusters.
pub async fn operator_by_prompt(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    match operator_or(&state, &headers).await {
        Ok(()) => query_by_prompt(&state, None, &window).await,
        Err(resp) => resp,
    }
}

/// GET /v1/dashboard/api/operator/alerts?days=7 — cost anomaly episodes
/// (ITEM-020 AC2: an operator view lists them, firing and cleared).
pub async fn operator_alerts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    let alerts = match crate::alerts::list_alerts(&state.db, window.days()).await {
        Ok(rows) => rows,
        Err(e) => return internal(&e.to_string()),
    };
    // ITEM-071: label a per-provider alert with the provider's name.
    let provider_ids: Vec<uuid::Uuid> = alerts
        .iter()
        .filter_map(|alert| provider_id_of(&alert.class))
        .collect();
    let provider_names: std::collections::HashMap<uuid::Uuid, String> = if provider_ids.is_empty() {
        std::collections::HashMap::new()
    } else {
        sqlx::query_as::<_, (uuid::Uuid, String)>(
            "SELECT id, name FROM providers WHERE id = ANY($1)",
        )
        .bind(&provider_ids)
        .fetch_all(&state.db)
        .await
        .map(|rows| rows.into_iter().collect())
        .unwrap_or_default()
    };
    let items: Vec<Value> = alerts
        .into_iter()
        .map(|a| {
            let provider_name = provider_id_of(&a.class)
                .and_then(|id| provider_names.get(&id).cloned());
            json!({
                "tenant_id": a.tenant_id.to_string(),
                "class": a.class,
                "status": a.status,
                "detail": a.detail,
                "threshold": a.threshold,
                "first_fired_at": a.first_fired_at,
                "last_fired_at": a.last_fired_at,
                "cleared_at": a.cleared_at,
                "provider_name": provider_name,
            })
        })
        .collect();
    json_ok(json!({"alerts": items, "window_days": window.days()}))
}

/// The provider id in a `provider_error:<id>` / `provider_latency:<id>` class.
fn provider_id_of(class: &str) -> Option<uuid::Uuid> {
    crate::alerts::class_entity(class, crate::alerts::PROVIDER_ERROR)
        .or_else(|| crate::alerts::class_entity(class, crate::alerts::PROVIDER_LATENCY))
}

/// GET /v1/dashboard/api/operator/guard?days=7 — platform-wide (ITEM-026 AC2).
pub async fn operator_guard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> Response {
    match operator_or(&state, &headers).await {
        Ok(()) => query_guard(&state, None, &window, "operator").await,
        Err(resp) => resp,
    }
}

/// GET /v1/dashboard/api/operator/keys?tenant_id=… — a tenant's key metadata
/// (ITEM-034 AC3). Key material is never returned — only lifecycle metadata.
pub async fn operator_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<KeyQuery>,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    match state.tenant_store.list_api_keys(query.tenant_id).await {
        Ok(keys) => {
            let rows: Vec<Value> = keys
                .iter()
                .map(|k| {
                    let mut value = key_json(k);
                    value["tenant_id"] = json!(k.tenant_id.to_string());
                    value
                })
                .collect();
            json_ok(json!({"keys": rows}))
        }
        Err(e) => internal(&e.to_string()),
    }
}

/// POST /v1/dashboard/api/operator/keys — mint a tenant key (ITEM-034 AC3).
/// The raw key is returned exactly once; only its hash is stored.
pub async fn operator_keys_create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<CreateKeyBody>,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    let tenant = match state.tenant_store.tenant_by_id(body.tenant_id).await {
        Ok(Some(tenant)) => tenant,
        Ok(None) => {
            return crate::json_error(
                StatusCode::NOT_FOUND,
                "tenant_not_found",
                "no such tenant",
            )
        }
        Err(e) => return internal(&e.to_string()),
    };
    let label = body.label.unwrap_or_else(|| "default".to_string());
    match mint_key_payload(&state, tenant.id, &label).await {
        Ok(payload) => json_ok(payload),
        Err(e) => internal(&e),
    }
}

/// POST /v1/dashboard/api/operator/keys/{key_id}/revoke — revoke a key
/// (ITEM-034 AC2: the next request with it is 401).
pub async fn operator_key_revoke(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_id): Path<uuid::Uuid>,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    match state.tenant_store.revoke_api_key(key_id).await {
        Ok(true) => json_ok(json!({"key_id": key_id.to_string(), "revoked": true})),
        Ok(false) => crate::json_error(
            StatusCode::NOT_FOUND,
            "key_not_found",
            "no such active key",
        ),
        Err(e) => internal(&e.to_string()),
    }
}

/// One `evolution_jobs` row (id, tenant id, adapter, reason, state, evidence,
/// created, updated).
type EvolutionRow = (String, String, String, String, String, Value, String, String);

/// GET /v1/dashboard/api/operator/evolution — every tenant's evolution jobs
/// (ITEM-031 AC4: drift detection and each later step are visible).
pub async fn operator_evolution(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    let rows: Vec<EvolutionRow> = match sqlx::query_as(
        "SELECT id::text, tenant_id::text, adapter, reason, state, evidence,
                created_at::text, updated_at::text
           FROM evolution_jobs
          ORDER BY created_at DESC
          LIMIT 200",
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) => rows,
        Err(e) => return internal(&e.to_string()),
    };
    let jobs: Vec<Value> = rows
        .into_iter()
        .map(
            |(id, tenant_id, adapter, reason, state, evidence, created_at, updated_at)| {
                json!({
                    "id": id,
                    "tenant_id": tenant_id,
                    "adapter": adapter,
                    "reason": reason,
                    "state": state,
                    "evidence": evidence,
                    "created_at": created_at,
                    "updated_at": updated_at,
                })
            },
        )
        .collect();
    json_ok(json!({ "jobs": jobs }))
}

/// POST /v1/dashboard/api/operator/evolution/detect — run a drift pass now;
/// the scheduled loop runs the same function every 15 minutes.
pub async fn operator_evolution_detect(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(resp) = operator_or(&state, &headers).await {
        return resp;
    }
    let queued = crate::evolution::run_evolution(&state.db).await;
    json_ok(json!({ "queued": queued }))
}
