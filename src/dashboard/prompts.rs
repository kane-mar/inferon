//! The prompt registry surface (ITEM-097): name a hash, list versions, diff two.
//!
//! Read-only in v1 — labels are additive, and nothing here edits a prompt or
//! deploys one. Versions are a view over `traces` ordered by first-seen, so the
//! registry never invents history that traffic did not produce.

use super::*;

/// `(name, hash, first_seen, last_seen, requests, cost, ok, thumbs_down)`.
type VersionRow = (String, String, String, String, i64, Decimal, i64, i64);

/// POST /v1/dashboard/api/prompts/labels — name a content hash (owner-only).
///
/// Naming is retroactive and idempotent: the same name and hash is a no-op, and
/// one hash may carry several names.
pub async fn label(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<LabelBody>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    let name = body.name.trim();
    let hash = body.prompt_hash.trim();
    if name.is_empty() || hash.is_empty() {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "invalid_label",
            "name and prompt_hash are required",
        );
    }
    let inserted = sqlx::query(
        "INSERT INTO prompt_labels (tenant_id, prompt_name, system_prompt_hash)
         VALUES ($1, $2, $3)
         ON CONFLICT (tenant_id, prompt_name, system_prompt_hash) DO NOTHING",
    )
    .bind(tenant_id)
    .bind(name)
    .bind(hash)
    .execute(&state.db)
    .await;
    match inserted {
        Ok(_) => json_ok(json!({ "name": name, "prompt_hash": hash })),
        Err(e) => internal(e.to_string()),
    }
}

/// DELETE /v1/dashboard/api/prompts/labels/:name/:hash — drop a label only.
pub async fn unlabel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, hash)): Path<(String, String)>,
) -> Response {
    let (tenant_id, role) = match session_membership(&state, &headers).await {
        Ok(membership) => membership,
        Err(resp) => return resp,
    };
    if let Err(resp) = owner_only(&role) {
        return resp;
    }
    let deleted = sqlx::query(
        "DELETE FROM prompt_labels WHERE tenant_id = $1 AND prompt_name = $2
           AND system_prompt_hash = $3",
    )
    .bind(tenant_id)
    .bind(&name)
    .bind(&hash)
    .execute(&state.db)
    .await;
    match deleted {
        Ok(result) => json_ok(json!({ "deleted": result.rows_affected() })),
        Err(e) => internal(e.to_string()),
    }
}

/// GET /v1/dashboard/api/prompts — named prompts, versions and their outcomes.
pub async fn prompts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let (tenant_id, role) = session_membership(&state, &headers).await?;

    // One row per (label, hash) that traffic actually produced, so a label on a
    // never-seen hash is listed with zero counts rather than hidden.
    let rows: Vec<VersionRow> = stored(sqlx::query_as(
        "SELECT l.prompt_name,
                l.system_prompt_hash,
                COALESCE(MIN(t.created_at), l.created_at)::text,
                COALESCE(MAX(t.created_at), l.created_at)::text,
                COUNT(t.id),
                COALESCE(SUM(t.upstream_cost_usd), 0),
                COUNT(t.id) FILTER (WHERE t.outcome = 'ok'),
                COUNT(down.trace_id)
         FROM prompt_labels l
         LEFT JOIN traces t
                ON t.tenant_id = l.tenant_id AND t.system_prompt_hash = l.system_prompt_hash
         LEFT JOIN (SELECT trace_id FROM trace_quality WHERE signal = 'down') down
                ON down.trace_id = t.id
         WHERE l.tenant_id = $1
         GROUP BY l.prompt_name, l.system_prompt_hash, l.created_at
         ORDER BY l.prompt_name, 3",
    )
    .bind(tenant_id)
    .fetch_all(&state.db)
    .await)?;

    let mut by_name: serde_json::Map<String, Value> = serde_json::Map::new();
    for (name, hash, first, last, requests, cost, ok, down) in rows {
        let entry = by_name
            .entry(name.clone())
            .or_insert_with(|| json!({ "name": name, "versions": [] }));
        if let Some(versions) = entry["versions"].as_array_mut() {
            versions.push(json!({
                "prompt_hash": hash,
                "first_seen": first,
                "last_seen": last,
                "requests": requests,
                "ok_rate": (requests > 0).then(|| ok as f64 / requests as f64),
                "thumbs_down": down,
                "spend_usd": cost.to_string(),
            }));
        }
    }

    // Content is owner-only and honours the capture switch (ITEM-088).
    let content = if role == "owner" {
        Some(state.hotpath.capture_enabled(&state, tenant_id).await)
    } else {
        None
    };
    Ok(json_ok(json!({
        "role": role,
        "content_available": content,
        "prompts": by_name.into_values().collect::<Vec<_>>(),
    })))
}

/// GET /v1/dashboard/api/prompts/diff?name=..&from=..&to=.. — what changed.
pub async fn diff(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<DiffQuery>,
) -> Result<Response, Response> {
    let (tenant_id, role) = session_membership(&state, &headers).await?;
    if role != "owner" {
        return Err(crate::json_error(
            StatusCode::FORBIDDEN,
            "owner_only",
            "only an owner can read prompt content",
        ));
    }
    if !state.hotpath.capture_enabled(&state, tenant_id).await {
        return Err(crate::json_error(
            StatusCode::BAD_REQUEST,
            "capture_disabled",
            "this tenant does not capture payloads, so there is no content to diff",
        ));
    }
    let labeled: Option<i64> = stored(
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM prompt_labels
             WHERE tenant_id = $1 AND prompt_name = $2
               AND system_prompt_hash IN ($3, $4)",
        )
        .bind(tenant_id)
        .bind(&query.name)
        .bind(&query.from)
        .bind(&query.to)
        .fetch_one(&state.db)
        .await,
    )?;
    if labeled.unwrap_or(0) < 2 {
        return Err(crate::json_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "both versions must be labelled with that name",
        ));
    }

    let previous = prompt_text(&state, tenant_id, &query.from).await?;
    let current = prompt_text(&state, tenant_id, &query.to).await?;
    let (Some(previous), Some(current)) = (previous, current) else {
        return Err(crate::json_error(
            StatusCode::NOT_FOUND,
            "content_unavailable",
            "one of the versions has no captured payload",
        ));
    };
    let changes = crate::promptreg::diff(&previous, &current);
    Ok(json_ok(json!({
        "name": query.name,
        "from": query.from,
        "to": query.to,
        "changes": changes.into_iter().map(|change| json!({
            "kind": change.kind.as_str(),
            "text": change.text,
        })).collect::<Vec<_>>(),
    })))
}

/// The system prompt captured for a hash, newest first.
async fn prompt_text(
    state: &AppState,
    tenant_id: uuid::Uuid,
    hash: &str,
) -> Result<Option<String>, Response> {
    let payload: Option<Value> = stored(
        sqlx::query_scalar(
            "SELECT raw_prompt FROM traces
             WHERE tenant_id = $1 AND system_prompt_hash = $2 AND raw_prompt IS NOT NULL
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(tenant_id)
        .bind(hash)
        .fetch_optional(&state.db)
        .await,
    )?;
    Ok(payload.and_then(|payload| {
        payload["messages"]
            .as_array()?
            .iter()
            .find(|message| message["role"] == "system")
            .and_then(|message| message["content"].as_str())
            .map(str::to_string)
    }))
}
