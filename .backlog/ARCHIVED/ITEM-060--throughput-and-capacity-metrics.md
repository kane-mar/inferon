## ITEM-060: Throughput & capacity metrics

**As an** operator
**I want** to see load, concurrency, and pipeline lag
**So that** I can size capacity and prove the hot-path latency claim under load

Trace counts don't show how hard the system is working right now. There is no
requests/min, no in-flight gauge, no telemetry queue depth, and no DB-flush lag —
so capacity decisions and the "sub-15ms hot path" claim are unmeasured.

**Acceptance Criteria:**
1. **Requests/min** (and a short rolling rate) are exposed, overall and per
   provider.
2. A **concurrency gauge** shows requests currently in flight.
3. **Telemetry queue depth** and **last-flush lag** (time from trace creation to
   persistence) are exposed; growing lag is visible before it becomes data loss.
4. The hot path's own latency (ITEM-056 overhead) can be read alongside load, so
   "does latency degrade with concurrency?" is answerable.
5. Metrics are read from the running process (an operator endpoint), not derived
   by heavy dashboard queries.

**Technical Notes:**
- A `Metrics` struct of atomics in `AppState`: in-flight counter (guard on the
  handler), request counter, channel depth (`tx` capacity minus receiver side),
  last-flush timestamp.
- Keep it allocation-free on the hot path; reading is operator-only.

**Source:** 2026-10-12 metrics review — no capacity or load visibility.

## Close-out (2026-10-13)

- `metrics.rs` adds `requests_total`, `requests_per_minute` (60s rolling window,
  time-ordered + pruned in place), `in_flight` (an `InFlightGuard` held for the
  request's processing) and `requests_by_provider` (`shared` = the platform key).
- `telemetry_queue_depth` / `telemetry_queue_capacity` are read from the live
  channel at the endpoint; `last_flush_age_ms` refreshes on each committed flush.
- Wiring: the in-flight guard in both proxy handlers and the edge ingest;
  `request_routed` after route resolution; `record_flush` in the Postgres sink.
- Exposed on the operator-only `GET /v1/dashboard/api/operator/metrics`.
- Rust **277 → 279** tests, clippy clean. Commit `6bec2f3`; CI `38478444597`
  (test+web+deploy) green.
- **Live**: a real request moved `requests_total` 1→2 and `requests_per_minute`
  1→2, `in_flight` returned to 0, `requests_by_provider` gained the provider id,
  `telemetry_queue_depth` 0/10000, and `last_flush_age_ms` refreshed (~3.9s→1.8s).
- Per-provider **latency/error** rates are ITEM-061; the proxy's own overhead is
  ITEM-066. The operator can read the hot path's latency percentiles (ITEM-056)
  from `/operator/summary` alongside this load signal.
