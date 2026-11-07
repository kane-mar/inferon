// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! In-process health metrics (ITEM-059 integrity counters + ITEM-060 throughput
//! & capacity).
//!
//! Monotonic counters and a few gauges that make the running process legible:
//! trace-pipeline integrity (queued/persisted/dropped), request throughput and
//! concurrency, and telemetry flush freshness. They reset on deploy — this is a
//! live health signal, not historical billing. Read from the process (an
//! operator endpoint), never by heavy queries.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

pub static TRACES_QUEUED: AtomicU64 = AtomicU64::new(0);
pub static TRACES_DROPPED_FULL: AtomicU64 = AtomicU64::new(0);
pub static TRACES_DROPPED_CLOSED: AtomicU64 = AtomicU64::new(0);
pub static TRACES_PERSISTED: AtomicU64 = AtomicU64::new(0);
pub static UNPRICED_MODELS: AtomicU64 = AtomicU64::new(0);

/// ITEM-060: total requests admitted, and how many are being processed now.
pub static REQUESTS_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static IN_FLIGHT: AtomicI64 = AtomicI64::new(0);
/// Wall-clock ms of the last committed telemetry flush (0 = never).
pub static LAST_FLUSH_MS: AtomicU64 = AtomicU64::new(0);

/// The rolling window for the requests/min rate.
const RATE_WINDOW_MS: u64 = 60_000;

/// Requests admitted per provider (`None` = the shared platform key).
static ROUTED_BY_PROVIDER: OnceLock<Mutex<HashMap<Option<Uuid>, u64>>> = OnceLock::new();

fn routed_by_provider() -> &'static Mutex<HashMap<Option<Uuid>, u64>> {
    ROUTED_BY_PROVIDER.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record that a request was routed to `provider_id` (after resolution).
pub fn request_routed(provider_id: Option<Uuid>) {
    let mut map = routed_by_provider()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *map.entry(provider_id).or_insert(0) += 1;
}

/// Outstanding dispatches per provider (ITEM-100 AC2).
static IN_FLIGHT_BY_PROVIDER: OnceLock<Mutex<HashMap<Option<Uuid>, i64>>> = OnceLock::new();

fn in_flight_by_provider() -> &'static Mutex<HashMap<Option<Uuid>, i64>> {
    IN_FLIGHT_BY_PROVIDER.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A guard for one provider dispatch; dropping it decrements the gauge.
pub struct ProviderGuard(Option<Uuid>);

impl Drop for ProviderGuard {
    fn drop(&mut self) {
        let mut map = in_flight_by_provider()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(count) = map.get_mut(&self.0) {
            *count = (*count - 1).max(0);
        }
    }
}

/// Record a dispatch to `provider_id`; hold the guard until it is answered.
pub fn provider_started(provider_id: Option<Uuid>) -> ProviderGuard {
    let mut map = in_flight_by_provider()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *map.entry(provider_id).or_insert(0) += 1;
    ProviderGuard(provider_id)
}

fn in_flight_json() -> serde_json::Value {
    let map = in_flight_by_provider()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut object = serde_json::Map::new();
    for (provider, count) in map.iter() {
        let key = provider.map(|id| id.to_string()).unwrap_or_else(|| "shared".to_string());
        object.insert(key, serde_json::json!(count));
    }
    serde_json::Value::Object(object)
}

fn routed_json() -> serde_json::Value {
    let map = routed_by_provider()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut object = serde_json::Map::new();
    for (provider, count) in map.iter() {
        let key = provider.map(|id| id.to_string()).unwrap_or_else(|| "shared".to_string());
        object.insert(key, serde_json::json!(count));
    }
    serde_json::Value::Object(object)
}

static REQUEST_TIMES: OnceLock<Mutex<VecDeque<u64>>> = OnceLock::new();

fn request_times() -> &'static Mutex<VecDeque<u64>> {
    REQUEST_TIMES.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// Drop the timestamps outside the window and return how many remain.
fn prune_and_count(times: &mut VecDeque<u64>, now_ms: u64) -> usize {
    while let Some(&front) = times.front() {
        if now_ms.saturating_sub(front) >= RATE_WINDOW_MS {
            times.pop_front();
        } else {
            break;
        }
    }
    times.len()
}

fn requests_per_minute(now_ms: u64) -> usize {
    let mut times = request_times()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    prune_and_count(&mut times, now_ms)
}

/// A guard that keeps the in-flight gauge accurate for the request's duration.
pub struct InFlightGuard(());

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Record the admission of a request; hold the returned guard while it is being
/// processed (drop = done).
pub fn request_started() -> InFlightGuard {
    IN_FLIGHT.fetch_add(1, Ordering::Relaxed);
    REQUESTS_TOTAL.fetch_add(1, Ordering::Relaxed);
    let now = crate::clock::now_ms();
    let mut times = request_times()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    times.push_back(now);
    prune_and_count(&mut times, now);
    InFlightGuard(())
}

pub fn count_queued() {
    TRACES_QUEUED.fetch_add(1, Ordering::Relaxed);
}

/// A trace that could not be queued: `full` = back-pressure, `false` = the
/// channel was closed (the worker is gone).
pub fn count_dropped(full: bool) {
    if full {
        TRACES_DROPPED_FULL.fetch_add(1, Ordering::Relaxed);
    } else {
        TRACES_DROPPED_CLOSED.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn count_persisted(count: usize) {
    if count > 0 {
        TRACES_PERSISTED.fetch_add(count as u64, Ordering::Relaxed);
    }
}

pub fn count_unpriced() {
    UNPRICED_MODELS.fetch_add(1, Ordering::Relaxed);
}

/// Record a committed telemetry flush (ITEM-060: flush freshness).
pub fn record_flush() {
    LAST_FLUSH_MS.store(crate::clock::now_ms(), Ordering::Relaxed);
}

/// A JSON snapshot for the operator endpoint (queue depth is added there, from
/// the live channel).
pub fn snapshot() -> serde_json::Value {
    let dropped_full = TRACES_DROPPED_FULL.load(Ordering::Relaxed);
    let dropped_closed = TRACES_DROPPED_CLOSED.load(Ordering::Relaxed);
    let now = crate::clock::now_ms();
    let last_flush = LAST_FLUSH_MS.load(Ordering::Relaxed);
    let last_flush_age_ms = (last_flush > 0).then(|| now.saturating_sub(last_flush));
    serde_json::json!({
        "traces_queued": TRACES_QUEUED.load(Ordering::Relaxed),
        "traces_persisted": TRACES_PERSISTED.load(Ordering::Relaxed),
        "traces_dropped_full": dropped_full,
        "traces_dropped_closed": dropped_closed,
        "lost": dropped_full + dropped_closed,
        "unpriced_models": UNPRICED_MODELS.load(Ordering::Relaxed),
        "requests_total": REQUESTS_TOTAL.load(Ordering::Relaxed),
        "requests_per_minute": requests_per_minute(now),
        "in_flight": IN_FLIGHT.load(Ordering::Relaxed),
        "requests_by_provider": routed_json(),
        "in_flight_by_provider": in_flight_json(),
        "last_flush_age_ms": last_flush_age_ms,
    })
}

#[cfg(test)]
mod tests;
