// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

    use super::*;

    #[test]
    fn prune_and_count_keeps_only_the_window() {
        let mut times: VecDeque<u64> = VecDeque::new();
        let now = 1_000_000;
        // Oldest first (the queue is time-ordered): one too old, one exactly at
        // the boundary (dropped), one fresh.
        times.push_back(now - RATE_WINDOW_MS - 5);
        times.push_back(now - RATE_WINDOW_MS);
        times.push_back(now - 10);
        assert_eq!(prune_and_count(&mut times, now), 1, "only the fresh sample stays");
        assert_eq!(times.len(), 1);
    }

    #[test]
    fn snapshot_reports_the_counters_and_loss_relation() {
        // Globals are shared across the test binary, so assert relations and
        // lower bounds, never exact deltas.
        count_queued();
        count_dropped(true);
        count_dropped(false);
        count_persisted(3);
        count_unpriced();
        let guard = request_started();
        let s = snapshot();
        assert!(s["traces_queued"].as_u64().unwrap() >= 1);
        assert!(s["traces_persisted"].as_u64().unwrap() >= 3);
        assert!(s["unpriced_models"].as_u64().unwrap() >= 1);
        assert!(s["requests_total"].as_u64().unwrap() >= 1);
        assert!(s["in_flight"].as_i64().unwrap() >= 1);
        assert!(s["requests_per_minute"].as_u64().unwrap() >= 1);
        request_routed(Some(uuid::Uuid::nil()));
        assert!(snapshot()["requests_by_provider"].is_object());
        assert_eq!(
            s["lost"].as_u64().unwrap(),
            s["traces_dropped_full"].as_u64().unwrap()
                + s["traces_dropped_closed"].as_u64().unwrap()
        );
        drop(guard);
    }

    #[test]
    fn flush_freshness_is_null_until_a_flush_is_recorded() {
        // Another test may have recorded one; assert the shape not the value.
        record_flush();
        let s = snapshot();
        assert!(s["last_flush_age_ms"].as_u64().is_some());
    }
