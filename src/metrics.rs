//! Telemetry-integrity counters (ITEM-059).
//!
//! In-process, monotonic counters that make the pipeline's health visible: how
//! many traces were queued, persisted, and dropped (and why), plus how many
//! models were costed at the default row. They reset on deploy — the value is a
//! live health signal, not historical billing.

use std::sync::atomic::{AtomicU64, Ordering};

pub static TRACES_QUEUED: AtomicU64 = AtomicU64::new(0);
pub static TRACES_DROPPED_FULL: AtomicU64 = AtomicU64::new(0);
pub static TRACES_DROPPED_CLOSED: AtomicU64 = AtomicU64::new(0);
pub static TRACES_PERSISTED: AtomicU64 = AtomicU64::new(0);
pub static UNPRICED_MODELS: AtomicU64 = AtomicU64::new(0);

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

/// A JSON snapshot for the operator endpoint.
pub fn snapshot() -> serde_json::Value {
    let dropped_full = TRACES_DROPPED_FULL.load(Ordering::Relaxed);
    let dropped_closed = TRACES_DROPPED_CLOSED.load(Ordering::Relaxed);
    serde_json::json!({
        "traces_queued": TRACES_QUEUED.load(Ordering::Relaxed),
        "traces_persisted": TRACES_PERSISTED.load(Ordering::Relaxed),
        "traces_dropped_full": dropped_full,
        "traces_dropped_closed": dropped_closed,
        "lost": dropped_full + dropped_closed,
        "unpriced_models": UNPRICED_MODELS.load(Ordering::Relaxed),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_reports_the_counters_and_loss_relation() {
        // Globals are shared across the test binary, so assert relations and
        // lower bounds, never exact deltas.
        count_queued();
        count_dropped(true);
        count_dropped(false);
        count_persisted(3);
        count_unpriced();
        let s = snapshot();
        assert!(s["traces_queued"].as_u64().unwrap() >= 1);
        assert!(s["traces_persisted"].as_u64().unwrap() >= 3);
        assert!(s["unpriced_models"].as_u64().unwrap() >= 1);
        assert_eq!(
            s["lost"].as_u64().unwrap(),
            s["traces_dropped_full"].as_u64().unwrap()
                + s["traces_dropped_closed"].as_u64().unwrap()
        );
    }
}
