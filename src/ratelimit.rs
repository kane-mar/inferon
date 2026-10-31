//! Velocity limits per API key (ITEM-095).
//!
//! Spend caps bound a month; this bounds a minute. One runaway client should
//! not be able to starve the others or burn the budget before anyone notices.
//!
//! The counters live in Postgres (`api_key_usage`, one row per key per minute),
//! incremented atomically with the check: restart-safe and correct across
//! instances, and skipped entirely for keys with no limits — the common case
//! pays nothing.
//!
//! Token accounting: a stream's usage is only known at the end, so the *prompt*
//! is estimated pre-flight (chars/4) and charged then; the completion is charged
//! when the trace is written. A request is rejected on the estimate, which is
//! what a client can act on before spending anything.

use serde::Serialize;
use uuid::Uuid;

/// A key's velocity limits. `0` = unlimited, on both axes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub requests_per_minute: i64,
    pub tokens_per_minute: i64,
}

impl Limits {
    pub fn is_unlimited(&self) -> bool {
        self.requests_per_minute <= 0 && self.tokens_per_minute <= 0
    }
}

/// What a request consumed in its window, after the check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub requests: i64,
    pub tokens: i64,
}

/// The verdict, with what the response headers need.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allowed(Usage),
    /// Which axis was hit, and how long until the window rolls over.
    Limited {
        axis: Axis,
        limit: i64,
        retry_after_secs: i64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Requests,
    Tokens,
}

impl Axis {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requests => "requests",
            Self::Tokens => "tokens",
        }
    }
}

/// Decide from the counters *after* this request was charged.
///
/// Charging first keeps the SQL to one atomic upsert; a rejected request has
/// therefore counted against the window (it used the check), which is the
/// behaviour clients expect from a fixed window.
pub fn decide(limits: Limits, usage: Usage, retry_after_secs: i64) -> Decision {
    if limits.requests_per_minute > 0 && usage.requests > limits.requests_per_minute {
        return Decision::Limited {
            axis: Axis::Requests,
            limit: limits.requests_per_minute,
            retry_after_secs,
        };
    }
    if limits.tokens_per_minute > 0 && usage.tokens > limits.tokens_per_minute {
        return Decision::Limited {
            axis: Axis::Tokens,
            limit: limits.tokens_per_minute,
            retry_after_secs,
        };
    }
    Decision::Allowed(usage)
}

/// Pre-flight token estimate for the limit: the prompt only, chars/4. The
/// completion is charged at trace time by `charge_tokens`.
pub fn estimate_tokens(prompt_chars: usize) -> i64 {
    (prompt_chars / 4) as i64
}

/// Seconds until the next minute boundary.
pub fn retry_after_secs(now_secs: i64) -> i64 {
    (60 - now_secs % 60).max(1)
}

/// Atomically charge this request and return the window's counters.
///
/// One statement, so two proxy instances cannot both see "limit - 1". Returns
/// `None` when the store cannot answer: a limiter outage must not take the data
/// plane down with it (fail-open, like every other switch).
pub async fn charge(
    pool: &sqlx::PgPool,
    key_id: Uuid,
    tokens: i64,
) -> Option<Usage> {
    let row: Result<(i64, i64), sqlx::Error> = sqlx::query_as(
        "INSERT INTO api_key_usage (api_key_id, window_start, requests, tokens)
         VALUES ($1, date_trunc('minute', NOW()), 1, $2)
         ON CONFLICT (api_key_id, window_start) DO UPDATE
           SET requests = api_key_usage.requests + 1,
               tokens = api_key_usage.tokens + EXCLUDED.tokens
         RETURNING requests, tokens",
    )
    .bind(key_id)
    .bind(tokens)
    .fetch_one(pool)
    .await;
    match row {
        Ok((requests, tokens)) => Some(Usage { requests, tokens }),
        Err(e) => {
            tracing::error!("rate-limit counter failed (allowing the request): {e}");
            None
        }
    }
}

/// Add the completion's tokens to the window already charged for the request.
/// Called from the telemetry path, best effort.
pub async fn charge_tokens(pool: &sqlx::PgPool, key_id: Uuid, tokens: i64) {
    if tokens <= 0 {
        return;
    }
    let updated = sqlx::query(
        "UPDATE api_key_usage SET tokens = tokens + $2
         WHERE api_key_id = $1 AND window_start = date_trunc('minute', NOW())",
    )
    .bind(key_id)
    .bind(tokens)
    .execute(pool)
    .await;
    if let Err(e) = updated {
        tracing::debug!("rate-limit token reconciliation skipped: {e}");
    }
}

#[cfg(test)]
mod tests;
