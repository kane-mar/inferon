//! Dashboard data plane: read-only aggregate views over `traces`, always scoped
//! to the authenticated tenant (AC5). No new write path (AC3).
//!
//! Pure JSON APIs consumed by the TypeScript frontend (`web/`) — the backend
//! renders no HTML. Tenant views live under `/v1/dashboard/api/*`; platform-wide
//! operator views under `/v1/dashboard/api/operator/*`.
//!
//! Structure: one module per surface (each owns its request bodies), plus two
//! pieces of shared plumbing — [`scope`] (who is calling) and [`respond`] (the
//! window parser, the success envelope and the one 500 policy).
//!
//! Every handler module starts with `use super::*`, which is why the shared
//! prelude below lives here rather than in each file.

mod audit_admin;
mod datasets;
pub mod evals;
mod keys;
mod members;
mod operator;
mod performance;
pub mod prompts;
mod providers;
mod queries;
mod respond;
mod routing;
mod scope;
mod sessions;

pub use audit_admin::*;
pub use datasets::*;
pub use keys::*;
pub use members::*;
pub use operator::*;
pub use performance::*;
pub use providers::*;
pub use queries::*;
pub use routing::*;
pub use sessions::*;

pub(crate) use respond::*;
pub(crate) use scope::*;

// The shared prelude every dashboard module inherits through `use super::*`.
use crate::clock::now_secs;
use crate::cost;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::sync::Arc;

#[cfg(test)]
mod tests;
