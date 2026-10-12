## ITEM-066: Proxy overhead split & latency regression alerts

**As an** operator
**I want** to separate the proxy's own time from the upstream model's, and be
alerted when tail latency regresses
**So that** a slow hop is attributable and a regression is caught before a
tenant reports it

Split from ITEM-056 (2026-10-12): percentiles alone (ITEM-056, done) do not say
**where** the time went, and nothing watches the tail.

**Acceptance Criteria:**
1. The API exposes the **proxy's own overhead** separately from upstream time —
   at minimum request-received → upstream-dispatch, and the non-generation
   remainder (`total − ttft` is generation, not overhead).
2. The dashboard marks the proxy-overhead share against the total.
3. A **p95 regression** (TTFT or total, per window) is alertable through the
   ITEM-020 alert path, with a per-tenant baseline so seasonality doesn't fire.

**Technical Notes:**
- Overhead needs two timestamps at the proxy seam (axum handler entry, upstream
  client dispatch); `ttft_ms` already straddles the upstream call. Store the
  overhead on the trace (idempotent migration) so it is queryable.
- Reuse the alert engine's episode model for latency thresholds/regressions.

**Source:** 2026-10-12 metrics review — split from ITEM-056.
