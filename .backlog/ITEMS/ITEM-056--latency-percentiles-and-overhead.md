## ITEM-056: Latency percentiles & proxy overhead

**As a** tenant for whom latency is the deciding constraint
**I want** tail latency and the proxy's own overhead, not just an average
**So that** I can trust the "performant" claim and catch regressions

The summary reports only the **mean** of `time_to_first_token_ms` and
`total_duration_ms`. Averages hide the tail that users actually feel, and there is
no way to tell how much time is the proxy vs the upstream model.

**Acceptance Criteria:**
1. The summary/timeseries expose **p50/p95/p99** for TTFT and total duration
   (not only the average), for a chosen window.
2. The API exposes the **proxy's own overhead** separately from upstream time
   (at minimum: request-received → upstream-dispatch, and the non-generation
   remainder), so a slow hop is attributable.
3. Percentiles are computed in Postgres (`percentile_cont`) or an equivalent
   streaming method — no in-process histogram state that a restart loses.
4. The dashboard renders latency with the tail visible (p95 headline) and marks
   the proxy-overhead share.
5. A regression in p95 is **alertable** (reuse the ITEM-020 alert path).

**Technical Notes:**
- `percentile_cont(0.95) WITHIN GROUP (ORDER BY total_duration_ms)` per window;
  tiny addition to `dashboard::query_summary`/timeseries.
- Proxy overhead needs two timestamps at the proxy seam (axum handler entry,
  upstream client dispatch); `ttft_ms` already straddles the upstream call.
- Keep the existing averages for backward compatibility; percentiles are additive.

**Source:** 2026-10-12 metrics review — latency is the PO's decidable constraint
(A+C chosen over B for it), yet only the mean is visible.
