//! Shared response shapes for the dashboard APIs: one window parser, one 500
//! policy, one success envelope.

use super::*;

/// Query params shared by the JSON APIs: `days` (window, default 7, clamp
/// 1..=365), `limit` (row cap for by-prompt, default 10) and `offset` (days
/// back the window ends; 0 = the current window, used for period comparison).
#[derive(Debug, serde::Deserialize)]
pub struct Window {
    pub(crate) days: Option<i64>,
    pub(crate) limit: Option<i64>,
    pub(crate) offset: Option<i64>,
}

impl Window {
    pub(crate) fn days(&self) -> i64 {
        self.days.unwrap_or(7).clamp(1, 365)
    }
    pub(crate) fn limit(&self) -> i64 {
        self.limit.unwrap_or(10).clamp(1, 100)
    }
    /// Days back from now where the window ends (0 = current, `days` = the
    /// immediately-preceding period). Lets a caller diff two periods.
    pub(crate) fn offset(&self) -> i64 {
        self.offset.unwrap_or(0).clamp(0, 3650)
    }
}

/// A 500 that keeps the detail in the log and returns a stable message — an
/// SQL or storage error is never echoed back to a tenant.
pub(crate) fn internal(detail: impl std::fmt::Display) -> Response {
    tracing::error!("dashboard request failed: {detail}");
    crate::json_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "dashboard_error",
        "the request could not be completed",
    )
}

pub(crate) fn json_ok(value: Value) -> Response {
    Json(value).into_response()
}

pub(crate) fn db_err(e: sqlx::Error) -> Response {
    internal(e.to_string())
}

pub(crate) fn default_true() -> bool {
    true
}

pub(crate) fn default_retention() -> i64 {
    30
}
