// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! Fallback & routing rules (ITEM-045/109): a tenant's ordered chain of steps
//! tried when the primary request fails. Each step names the failures it
//! handles (transport, 429, 5xx) and, optionally, the schema guard. An empty
//! chain is the default and changes nothing.

use serde_json::Value;
use uuid::Uuid;

pub use crate::StoreResult;

/// Failure classes that trigger the next chain step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackReason {
    Transport,
    RateLimited,
    ServerError,
}

impl FallbackReason {
    pub fn label(self) -> &'static str {
        match self {
            FallbackReason::Transport => "upstream_transport",
            FallbackReason::RateLimited => "upstream_429",
            FallbackReason::ServerError => "upstream_5xx",
        }
    }
}

/// What a chain step handles. `Guard` is the schema guard's failed validation;
/// the other three are the retryable upstream failures ITEM-045 shipped.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum FallbackTrigger {
    Transport,
    RateLimited,
    ServerError,
    Guard,
}

impl FallbackTrigger {
    /// The retryable upstream failures, without the guard — what a legacy chain
    /// (a bare list of models) means.
    pub const RETRYABLE: [Self; 3] = [Self::Transport, Self::RateLimited, Self::ServerError];
}

impl From<FallbackReason> for FallbackTrigger {
    fn from(reason: FallbackReason) -> Self {
        match reason {
            FallbackReason::Transport => Self::Transport,
            FallbackReason::RateLimited => Self::RateLimited,
            FallbackReason::ServerError => Self::ServerError,
        }
    }
}

/// One chain step: a model and the failures it may be used for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FallbackStep {
    pub model: String,
    pub triggers: Vec<FallbackTrigger>,
}

impl FallbackStep {
    pub fn new(model: impl Into<String>, triggers: &[FallbackTrigger]) -> Self {
        Self {
            model: model.into(),
            triggers: triggers.to_vec(),
        }
    }

    /// The pre-109 behaviour: every retryable upstream failure, no guard.
    pub fn retryable(model: impl Into<String>) -> Self {
        Self::new(model, &FallbackTrigger::RETRYABLE)
    }

    /// A stored step with no triggers predates the field: treat it as the old
    /// retryable-only behaviour. Only the read path normalizes; a submitted
    /// empty list is a validation error.
    fn normalized(mut self) -> Self {
        if self.triggers.is_empty() {
            self.triggers = FallbackTrigger::RETRYABLE.to_vec();
        }
        self
    }
}

/// Stored form: the old shape is a bare model string (all triggers); the new
/// one is `{model, triggers}`. A missing/empty trigger list means all, so a
/// chain written by an older build keeps working.
impl serde::Serialize for FallbackStep {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut step = serializer.serialize_struct("FallbackStep", 2)?;
        step.serialize_field("model", &self.model)?;
        step.serialize_field("triggers", &self.triggers)?;
        step.end()
    }
}

impl<'de> serde::Deserialize<'de> for FallbackStep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Legacy(String),
            Step {
                model: String,
                #[serde(default)]
                triggers: Option<Vec<FallbackTrigger>>,
            },
        }

        Ok(match Raw::deserialize(deserializer)? {
            Raw::Legacy(model) => Self::retryable(model),
            Raw::Step { model, triggers } => Self {
                model,
                triggers: triggers.unwrap_or_else(|| FallbackTrigger::RETRYABLE.to_vec()),
            },
        })
    }
}

/// A stored chain, tolerating malformed entries: the read path must never fail
/// the request path, so an item that is not a step is dropped.
pub fn parse_steps(value: &Value) -> Vec<FallbackStep> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| serde_json::from_value::<FallbackStep>(item.clone()).ok())
        .map(FallbackStep::normalized)
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepIssue {
    EmptyModel,
    EmptyTriggers,
    DuplicateModel(String),
}

impl std::fmt::Display for StepIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepIssue::EmptyModel => write!(f, "a fallback step needs a model"),
            StepIssue::EmptyTriggers => write!(f, "a fallback step needs at least one trigger"),
            StepIssue::DuplicateModel(model) => write!(f, "`{model}` appears twice in the chain"),
        }
    }
}

impl std::error::Error for StepIssue {}

/// Save-time validation: a chain with an empty step or a duplicate model is a
/// typo, not a rule.
pub fn validate_steps(steps: &[FallbackStep]) -> Result<(), StepIssue> {
    let mut seen = std::collections::HashSet::new();
    for step in steps {
        let model = step.model.trim();
        if model.is_empty() {
            return Err(StepIssue::EmptyModel);
        }
        if step.triggers.is_empty() {
            return Err(StepIssue::EmptyTriggers);
        }
        if !seen.insert(model.to_string()) {
            return Err(StepIssue::DuplicateModel(model.to_string()));
        }
    }
    Ok(())
}

/// The first step after `from` that handles `trigger` — steps that do not are
/// skipped, so a 429 can pass a 5xx-only step instead of stopping the chain.
pub fn next_handling(
    triggers: &[&[FallbackTrigger]],
    from: usize,
    trigger: FallbackTrigger,
) -> Option<usize> {
    (from + 1..triggers.len()).find(|&index| triggers[index].contains(&trigger))
}

/// Every step after `from` that handles `trigger`, in chain order. The guard
/// replays through these; the HTTP fallback takes the first.
pub fn all_handling(
    triggers: &[&[FallbackTrigger]],
    from: usize,
    trigger: FallbackTrigger,
) -> Vec<usize> {
    (from + 1..triggers.len())
        .filter(|&index| triggers[index].contains(&trigger))
        .collect()
}

/// A 429 or 5xx (or a transport error) is retryable; a 4xx that isn't 429 is a
/// request problem and is surfaced as-is. `None` = not retryable.
pub fn fallback_reason(status: u16) -> Option<FallbackReason> {
    match status {
        429 => Some(FallbackReason::RateLimited),
        code if (500..600).contains(&code) => Some(FallbackReason::ServerError),
        _ => None,
    }
}

#[async_trait::async_trait]
pub trait RoutingStore: Send + Sync {
    /// The tenant's ordered fallback steps (empty = no chain).
    async fn fallbacks(&self, tenant_id: Uuid) -> StoreResult<Vec<FallbackStep>>;
    async fn set_fallbacks(&self, tenant_id: Uuid, steps: &[FallbackStep]) -> StoreResult<()>;
}

// --- Postgres ---------------------------------------------------------------

pub struct PostgresRoutingStore {
    pool: sqlx::PgPool,
}

impl PostgresRoutingStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl RoutingStore for PostgresRoutingStore {
    async fn fallbacks(&self, tenant_id: Uuid) -> StoreResult<Vec<FallbackStep>> {
        let row: Option<(sqlx::types::Json<Value>,)> =
            sqlx::query_as("SELECT fallbacks FROM routing_policies WHERE tenant_id = $1")
                .bind(tenant_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(json,)| parse_steps(&json.0)).unwrap_or_default())
    }

    async fn set_fallbacks(&self, tenant_id: Uuid, steps: &[FallbackStep]) -> StoreResult<()> {
        validate_steps(steps)?;
        sqlx::query(
            "INSERT INTO routing_policies (tenant_id, fallbacks) VALUES ($1, $2)
             ON CONFLICT (tenant_id)
             DO UPDATE SET fallbacks = $2, updated_at = NOW()",
        )
        .bind(tenant_id)
        .bind(sqlx::types::Json(serde_json::json!(steps)))
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

// --- In-memory (tests) ------------------------------------------------------

#[derive(Default)]
pub struct InMemoryRoutingStore {
    rows: std::sync::Mutex<Vec<(Uuid, Vec<FallbackStep>)>>,
}

impl InMemoryRoutingStore {
    /// A legacy chain: bare models, every trigger.
    pub fn with_fallbacks(tenant_id: Uuid, models: &[&str]) -> Self {
        Self::with_steps(
            tenant_id,
            &models
                .iter()
                .map(|model| FallbackStep::retryable(*model))
                .collect::<Vec<_>>(),
        )
    }

    pub fn with_steps(tenant_id: Uuid, steps: &[FallbackStep]) -> Self {
        Self {
            rows: std::sync::Mutex::new(vec![(tenant_id, steps.to_vec())]),
        }
    }
}

#[async_trait::async_trait]
impl RoutingStore for InMemoryRoutingStore {
    async fn fallbacks(&self, tenant_id: Uuid) -> StoreResult<Vec<FallbackStep>> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .find(|(id, _)| *id == tenant_id)
            .map(|(_, steps)| steps.clone())
            .unwrap_or_default())
    }

    async fn set_fallbacks(&self, tenant_id: Uuid, steps: &[FallbackStep]) -> StoreResult<()> {
        validate_steps(steps)?;
        let mut rows = self.rows.lock().unwrap();
        match rows.iter_mut().find(|(id, _)| *id == tenant_id) {
            Some((_, existing)) => *existing = steps.to_vec(),
            None => rows.push((tenant_id, steps.to_vec())),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
