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
