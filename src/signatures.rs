//! Semantic task signatures for dataset grouping (ITEM-089).
//!
//! Grouping by `system_prompt_hash` is exact-string grouping: a prompt that
//! differs by a tenant name is a different dataset. This module turns the task
//! *text* into an embedder vector and clusters those vectors, so near-identical
//! tasks land in one cluster.
//!
//! Everything here is pure: the embedding and the database live with the
//! caller (`run_pass`), which keeps the clustering deterministic and testable
//! without ONNX. Without an embedder the labels degrade to the exact hash
//! (ITEM-089 AC4) — grouping never fails, it just stops being semantic.

use serde_json::Value;

use crate::embed::EMBED_DIM;

/// Version of the signature pipeline: the model, the width, and the text
/// selection rule. A signature is only comparable within one version.
pub const SIGNATURE_VERSION: &str = "bge-micro-v2/384/v1";
/// Cosine similarity at or above which two prompts are the same task.
///
/// BGE-micro-v2 normalizes its vectors, so the threshold is a plain cosine.
/// 0.92 was chosen on the synthetic corpus in the tests: paraphrases of the
/// same instruction measure 0.95–0.99, unrelated tasks measure below 0.6.
pub const CLUSTER_THRESHOLD: f32 = 0.92;
/// Prompts embedded per periodic pass. One embed is ~150 ms on the production
/// tier, so a pass stays well under a minute and never runs in the hot path.
pub const PASS_BATCH: i64 = 50;

/// Which turn the task text came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    System,
    User,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
        }
    }
}

/// The text a task signature is computed over: the system prompt, or the first
/// user turn when the request has none.
pub fn task_text(payload: &Value) -> Option<(&str, Source)> {
    let messages = payload["messages"].as_array()?;
    let content = |role: &str| {
        messages
            .iter()
            .find(|message| message["role"] == role)
            .and_then(|message| message["content"].as_str())
            .filter(|text| !text.trim().is_empty())
    };
    if let Some(text) = content("system") {
        return Some((text, Source::System));
    }
    content("user").map(|text| (text, Source::User))
}

/// Cosine similarity. The embedder normalizes its vectors, so this is a dot
/// product; the norms are kept for callers passing raw vectors.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na * nb)
}

/// Deterministic leader clustering: walk the prompts in key order and join the
/// first representative within `threshold`, else become one.
///
/// `keys` are stable identities (prompt hashes) and the result is aligned to
/// the input order. The label is the representative's key, not its index, so a
/// later pass over a superset re-uses the same labels instead of renumbering
/// clusters — datasets do not jump around between runs.
pub fn assign_clusters(
    keys: &[String],
    vectors: &[[f32; EMBED_DIM]],
    threshold: f32,
) -> Vec<String> {
    assert_eq!(keys.len(), vectors.len(), "one vector per key");
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by(|&a, &b| keys[a].cmp(&keys[b]));

    let mut representatives: Vec<(&str, [f32; EMBED_DIM])> = Vec::new();
    let mut labels: Vec<String> = keys.to_vec();
    for index in order {
        let joined = representatives
            .iter()
            .find(|(_, vector)| cosine(vector, &vectors[index]) >= threshold);
        match joined {
            Some((key, _)) => labels[index] = (*key).to_string(),
            None => representatives.push((&keys[index], vectors[index])),
        }
    }
    labels
}

/// The embedder that produces the vectors; part of the signature's identity.
pub const EMBED_MODEL: &str = "TaylorAI/bge-micro-v2";

/// One prompt waiting for a signature: a tenant's distinct task text, when the
/// payload was captured at all (a tenant with capture off still gets a label —
/// its exact hash — so the pass can never starve on unlabellable rows).
struct Pending {
    prompt_hash: String,
    text: Option<(String, Source)>,
}

/// Embed this pass's unclustered prompts and label their traces.
///
/// Bounded by [`PASS_BATCH`] and off the request path: the proxy never embeds.
/// With no embedder (or no captured payload) the label is the prompt hash
/// itself, so grouping still works and the pass drains instead of retrying the
/// same prompts forever. Returns the number of prompts labelled.
pub async fn run_pass(pool: &sqlx::PgPool, embedder: &dyn crate::embed::Embedder) -> usize {
    // One row per distinct prompt. `MAX(raw_prompt::text)` picks any captured
    // payload for it; a prompt that was never captured comes back NULL.
    let rows = sqlx::query_as::<_, (uuid::Uuid, String, Option<String>)>(
        "SELECT t.tenant_id, t.system_prompt_hash, MAX(t.raw_prompt::text)
         FROM traces t
         WHERE t.task_cluster IS NULL
           AND t.system_prompt_hash IS NOT NULL
         GROUP BY t.tenant_id, t.system_prompt_hash
         ORDER BY t.tenant_id, t.system_prompt_hash
         LIMIT $1",
    )
    .bind(PASS_BATCH)
    .fetch_all(pool)
    .await;

    let rows = match rows {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!("task signatures: prompt lookup failed: {e}");
            return 0;
        }
    };

    let mut pending: std::collections::HashMap<uuid::Uuid, Vec<Pending>> =
        std::collections::HashMap::new();
    for (tenant_id, prompt_hash, raw_prompt) in rows {
        let payload = raw_prompt
            .as_deref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok());
        let text = payload
            .as_ref()
            .and_then(task_text)
            .map(|(text, source)| (text.to_string(), source));
        pending
            .entry(tenant_id)
            .or_default()
            .push(Pending { prompt_hash, text });
    }

    let mut labelled = 0usize;
    for (tenant_id, prompts) in pending {
        labelled += label_tenant(pool, tenant_id, &prompts, embedder).await;
    }
    if labelled > 0 {
        tracing::info!("task signatures: labelled {labelled} prompt(s)");
    }
    labelled
}

/// Cluster one tenant's pending prompts and write the labels. Returns how many
/// prompts were labelled (a store error is logged, never fatal).
async fn label_tenant(
    pool: &sqlx::PgPool,
    tenant_id: uuid::Uuid,
    prompts: &[Pending],
    embedder: &dyn crate::embed::Embedder,
) -> usize {
    let vectors: Vec<Option<[f32; crate::embed::EMBED_DIM]>> = prompts
        .iter()
        .map(|prompt| prompt.text.as_ref().and_then(|(text, _)| embedder.embed(text)))
        .collect();

    let embedded: Vec<([f32; crate::embed::EMBED_DIM], &Pending)> = prompts
        .iter()
        .zip(&vectors)
        .filter_map(|(prompt, vector)| vector.map(|vector| (vector, prompt)))
        .collect();

    // Whatever cannot be embedded keeps the exact hash as its label (AC4).
    let unembedded: Vec<(&String, &String)> = prompts
        .iter()
        .zip(&vectors)
        .filter(|(_, vector)| vector.is_none())
        .map(|(prompt, _)| (&prompt.prompt_hash, &prompt.prompt_hash))
        .collect();
    if !unembedded.is_empty() {
        set_cluster(pool, tenant_id, &unembedded, "hash").await;
    }
    if embedded.is_empty() {
        return unembedded.len();
    }

    let keys: Vec<String> = embedded.iter().map(|(_, p)| p.prompt_hash.clone()).collect();
    let matrix: Vec<[f32; crate::embed::EMBED_DIM]> = embedded.iter().map(|(v, _)| *v).collect();
    let labels = assign_clusters(&keys, &matrix, CLUSTER_THRESHOLD);

    for ((vector, prompt), label) in embedded.iter().zip(&labels) {
        let source = prompt.text.as_ref().map_or(Source::System, |(_, source)| *source);
        let stored = sqlx::query(
            "INSERT INTO task_signatures
                (tenant_id, prompt_hash, signature_version, embed_model, dim, source, vector,
                 cluster_label, cluster_source)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'embedding')
             ON CONFLICT (tenant_id, prompt_hash, signature_version) DO UPDATE
               SET vector = EXCLUDED.vector, cluster_label = EXCLUDED.cluster_label,
                   cluster_source = 'embedding', updated_at = NOW()",
        )
        .bind(tenant_id)
        .bind(&prompt.prompt_hash)
        .bind(SIGNATURE_VERSION)
        .bind(EMBED_MODEL)
        .bind(crate::embed::EMBED_DIM as i32)
        .bind(source.as_str())
        .bind(vector.to_vec())
        .bind(label)
        .execute(pool)
        .await;
        if let Err(e) = stored {
            tracing::error!("task signatures: store failed: {e}");
        }
    }

    let pairs: Vec<(&String, &String)> = keys.iter().zip(&labels).collect();
    set_cluster(pool, tenant_id, &pairs, "embedding").await;
    embedded.len() + unembedded.len()
}

/// Point this tenant's unclustered traces at their labels.
async fn set_cluster(
    pool: &sqlx::PgPool,
    tenant_id: uuid::Uuid,
    pairs: &[(&String, &String)],
    source: &str,
) -> usize {
    let hashes: Vec<String> = pairs.iter().map(|(hash, _)| (*hash).clone()).collect();
    let labels: Vec<String> = pairs.iter().map(|(_, label)| (*label).clone()).collect();
    let updated = sqlx::query(
        "UPDATE traces t
         SET task_cluster = labels.label, task_cluster_source = $3
         FROM UNNEST($2::text[], $4::text[]) AS labels(hash, label)
         WHERE t.tenant_id = $1
           AND t.system_prompt_hash = labels.hash
           AND t.task_cluster IS NULL",
    )
    .bind(tenant_id)
    .bind(&hashes)
    .bind(source)
    .bind(&labels)
    .execute(pool)
    .await;
    match updated {
        Ok(result) => result.rows_affected() as usize,
        Err(e) => {
            tracing::error!("task signatures: trace update failed: {e}");
            0
        }
    }
}

#[cfg(test)]
mod tests;
