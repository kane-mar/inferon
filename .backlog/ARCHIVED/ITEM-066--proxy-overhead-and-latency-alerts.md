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

## Close-out (2026-10-14)

- `traces.proxy_ms` (migration 031): the proxy's own pre-upstream time
  (request-received → upstream dispatch: auth, parse, routing, budget, affinity,
  policy, provider resolution), measured at the dispatch seam in both handlers.
- `/api/summary` exposes `proxy_p50_ms` / `proxy_p95_ms` alongside the TTFT and
  total percentiles, so a request splits into **proxy overhead**, upstream wait
  (`ttft − proxy`), and generation (`total − ttft`).
- The dashboard's p95 card shows the **proxy-overhead share** as a hint.
- Rust **293 → 294** tests, clippy clean; tsc/eslint/i18n green. Commit `4baa606`;
  CI `38515823276` (test+web+deploy) green.
- **Live**: a `deepseek-flash` request traced `proxy_ms = 26` against a total of
  1172 ms (≈2% — the hop is not the bottleneck); the summary reported
  `proxy_p95_ms = 26` vs `ttft_p95_ms = 1626` and `duration_p95_ms = 1666`.
- **AC3 (p95 regression alerts) is folded into ITEM-068**, which is now the single
  "metric-regression alerting" item (error rate, per-provider health, latency).
