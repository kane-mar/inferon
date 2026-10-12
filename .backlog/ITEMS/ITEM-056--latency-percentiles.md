## ITEM-056: Latency percentiles

**As a** tenant for whom latency is the deciding constraint
**I want** tail latency, not just an average
**So that** I can trust the "performant" claim and catch regressions

The summary reports only the **mean** of `time_to_first_token_ms` and
`total_duration_ms`. Averages hide the tail that users actually feel.

**Acceptance Criteria:**
1. The summary exposes **p50/p95/p99** for TTFT and total duration (not only the
   average), for a chosen window — tenant and operator scopes alike.
2. Percentiles are computed in Postgres (`percentile_cont`) — no in-process
   histogram state that a restart loses.
3. The dashboard renders latency with the tail visible (p95 headline, the average
   shown as a hint).
4. The existing averages remain (additive change).

**Technical Notes:**
- Implemented as a second aggregate query (`LatencyRow`) because sqlx's tuple
  `FromRow` tops out at 16 columns; the summary tuple is unchanged.
- Proxy-overhead attribution and p95 regression alerts are split to **ITEM-066**.

**Source:** 2026-10-12 metrics review — latency is the PO's decidable constraint
(A+C chosen over B for it), yet only the mean was visible.
