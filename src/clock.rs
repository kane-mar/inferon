// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! Wall-clock helpers — one definition of "now" for the hot path and the
//! background jobs, so the modules cannot drift on time bases.

use std::time::{SystemTime, UNIX_EPOCH};

/// Unix seconds. `0` if the clock is before the epoch (unreachable in practice —
/// the fallback keeps callers total).
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Unix seconds as a signed value (trace and affinity timestamps).
pub fn now_secs_i64() -> i64 {
    now_secs() as i64
}

/// Unix milliseconds.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
