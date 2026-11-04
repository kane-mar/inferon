//! ITEM-108: append-only audit of owner-only control-plane changes.
//!
//! `config_audit` records who changed a routing/budget setting, when, and the
//! before/after value. It is deliberately separate from the policy store
//! (ITEM-064 versions adaptive-policy decisions): this is the tenant's own
//! control plane, and its changes must be attributable to a person.

use crate::budget::{ConfigUpdate, TenantBudget};
use serde_json::Value;
use uuid::Uuid;

pub use crate::StoreResult;

/// Who made the change. `login` is a snapshot, so a deleted user still reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Actor {
    pub id: Uuid,
    pub login: String,
}

/// One changed setting. `None` on either side = unset.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub setting: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

/// A stored event, as the dashboard reads it back.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AuditRecord {
    pub actor_login: String,
    pub setting: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
    pub at: String,
}

/// A change when the two serialized values differ; `None` when they are equal.
pub fn change<T: serde::Serialize>(setting: &str, before: &T, after: &T) -> Option<Change> {
    let before = serde_json::to_value(before).ok()?;
    let after = serde_json::to_value(after).ok()?;
    if before == after {
        return None;
    }
    Some(Change {
        setting: setting.to_string(),
        before: Some(before),
        after: Some(after),
    })
}

/// The settings a `POST /api/config` actually edits, in the order the UI shows
/// them. An unchanged save produces no events.
pub fn config_changes(before: &TenantBudget, after: &ConfigUpdate) -> Vec<Change> {
    let mut changes = Vec::new();
    let mut push = |change: Option<Change>| {
        if let Some(change) = change {
            changes.push(change);
        }
    };

    push(change(
        "budget_limit_cents",
        &before.limit_cents,
        &after.budget_limit_cents,
    ));
    push(change(
        "cheap_fallback_model",
        &before.cheap_fallback_model,
        &after.cheap_fallback_model,
    ));
    push(change(
        "guard_enabled",
        &before.guard_enabled,
        &after.guard_enabled,
    ));
    push(change(
        "router_enabled",
        &before.router_enabled,
        &after.router_enabled,
    ));
    push(change(
        "router_threshold",
        &before.router_threshold,
        &after.router_threshold,
    ));
    push(change(
        "capture_payloads",
        &before.capture_payloads,
        &after.capture_payloads,
    ));
    push(change(
        "payload_retention_days",
        &before.payload_retention_days,
        &after.payload_retention_days,
    ));
    push(change(
        "lint_enabled",
        &before.lint_enabled,
        &after.lint_enabled,
    ));
    push(change("lint_rules", &before.lint_rules, &after.lint_rules));
    push(change(
        "translate_surfaces",
        &before.translate_surfaces,
        &after.translate_surfaces,
    ));
    push(change("rpm_limit", &before.rpm_limit, &after.rpm_limit));
    push(change("tpm_limit", &before.tpm_limit, &after.tpm_limit));
    push(change(
        "pii_detection",
        &before.pii_detection,
        &after.pii_detection,
    ));
    push(change(
        "pii_redact_storage",
        &before.pii_redact_storage,
        &after.pii_redact_storage,
    ));
    push(change(
        "pii_redact_wire",
        &before.pii_redact_wire,
        &after.pii_redact_wire,
    ));
    changes
}

#[async_trait::async_trait]
pub trait ConfigAuditStore: Send + Sync {
    /// Append the changes for one mutation. An empty slice is a no-op.
    async fn record(&self, tenant_id: Uuid, actor: &Actor, changes: &[Change]) -> StoreResult<()>;
    /// The most recent events for a tenant, newest first.
    async fn recent(&self, tenant_id: Uuid, limit: usize) -> StoreResult<Vec<AuditRecord>>;
}

// --- Postgres ---------------------------------------------------------------

pub struct PostgresConfigAuditStore {
    pool: sqlx::PgPool,
}

impl PostgresConfigAuditStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

struct AuditRow {
    actor_login: String,
    setting: String,
    before_value: Option<Value>,
    after_value: Option<Value>,
    at: String,
}

impl sqlx::FromRow<'_, sqlx::postgres::PgRow> for AuditRow {
    fn from_row(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(Self {
            actor_login: row.try_get("actor_login")?,
            setting: row.try_get("setting")?,
            before_value: row.try_get("before_value")?,
            after_value: row.try_get("after_value")?,
            at: row.try_get("at")?,
        })
    }
}

#[async_trait::async_trait]
impl ConfigAuditStore for PostgresConfigAuditStore {
    async fn record(&self, tenant_id: Uuid, actor: &Actor, changes: &[Change]) -> StoreResult<()> {
        if changes.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for change in changes {
            sqlx::query(
                "INSERT INTO config_audit
                    (tenant_id, actor_id, actor_login, setting, before_value, after_value)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(tenant_id)
            .bind(actor.id)
            .bind(&actor.login)
            .bind(&change.setting)
            .bind(change.before.as_ref())
            .bind(change.after.as_ref())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn recent(&self, tenant_id: Uuid, limit: usize) -> StoreResult<Vec<AuditRecord>> {
        let rows: Vec<AuditRow> = sqlx::query_as(
            "SELECT actor_login, setting, before_value, after_value,
                    to_char(recorded_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS at
             FROM config_audit
             WHERE tenant_id = $1
             ORDER BY recorded_at DESC, id DESC
             LIMIT $2",
        )
        .bind(tenant_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| AuditRecord {
                actor_login: row.actor_login,
                setting: row.setting,
                before: row.before_value,
                after: row.after_value,
                at: row.at,
            })
            .collect())
    }
}

// --- In-memory (tests) ------------------------------------------------------

#[derive(Default)]
pub struct InMemoryConfigAuditStore {
    rows: std::sync::Mutex<Vec<(Uuid, AuditRecord)>>,
}

#[async_trait::async_trait]
impl ConfigAuditStore for InMemoryConfigAuditStore {
    async fn record(&self, tenant_id: Uuid, actor: &Actor, changes: &[Change]) -> StoreResult<()> {
        let mut rows = self.rows.lock().unwrap();
        for change in changes {
            rows.push((
                tenant_id,
                AuditRecord {
                    actor_login: actor.login.clone(),
                    setting: change.setting.clone(),
                    before: change.before.clone(),
                    after: change.after.clone(),
                    at: crate::clock::now_secs().to_string(),
                },
            ));
        }
        Ok(())
    }

    async fn recent(&self, tenant_id: Uuid, limit: usize) -> StoreResult<Vec<AuditRecord>> {
        let rows = self.rows.lock().unwrap();
        Ok(rows
            .iter()
            .rev()
            .filter(|(id, _)| *id == tenant_id)
            .take(limit)
            .map(|(_, record)| record.clone())
            .collect())
    }
}

#[cfg(test)]
mod tests;
