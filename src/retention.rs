//! Payload retention (ITEM-088).
//!
//! Costs, outcomes and aggregates are kept forever; raw prompts/completions and
//! their dataset qualification are not. The periodic pass applies each tenant's
//! `payload_retention_days` (default 30, `0` = keep), in bounded batches so a
//! large backlog never holds one long lock. An owner can purge on demand.

use sqlx::PgPool;
use uuid::Uuid;

/// Rows per statement: bounded so the purge never blocks the proxy's reads.
const BATCH: i64 = 5_000;

const NULL_PAYLOADS: &str = "raw_prompt = NULL, raw_completion = NULL,
    example_state = NULL, example_reason = NULL, completion_hash = NULL";

/// Purge payloads past their tenant's retention window. Returns rows purged.
pub async fn purge_expired(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut total = 0u64;
    loop {
        let result = sqlx::query(&format!(
            "WITH expired AS (
                 SELECT t.id FROM traces t
                 LEFT JOIN tenant_configurations c ON c.tenant_id = t.tenant_id
                 WHERE (t.raw_prompt IS NOT NULL OR t.raw_completion IS NOT NULL)
                   AND COALESCE(c.payload_retention_days, 30) > 0
                   AND t.created_at < NOW()
                       - (COALESCE(c.payload_retention_days, 30) * INTERVAL '1 day')
                 LIMIT $1
             )
             UPDATE traces SET {NULL_PAYLOADS}
             WHERE id IN (SELECT id FROM expired)"
        ))
        .bind(BATCH)
        .execute(pool)
        .await?;
        total += result.rows_affected();
        if result.rows_affected() < BATCH as u64 {
            return Ok(total);
        }
    }
}

/// Purge every stored payload for one tenant, regardless of age. Returns rows
/// purged. Used by the owner's on-demand purge.
pub async fn purge_tenant(pool: &PgPool, tenant_id: Uuid) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(&format!(
        "UPDATE traces SET {NULL_PAYLOADS}
         WHERE tenant_id = $1
           AND (raw_prompt IS NOT NULL OR raw_completion IS NOT NULL)"
    ))
    .bind(tenant_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests;
