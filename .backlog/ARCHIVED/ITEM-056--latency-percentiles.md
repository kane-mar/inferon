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

## Close-out (2026-10-12)

- `dashboard::query_summary` now returns `ttft_p50/p95/p99_ms` and
  `duration_p50/p95/p99_ms` via a Postgres `percentile_cont` query, for both the
  tenant and operator scopes; the averages are unchanged.
- The dashboard latency card shows **p95** as the headline with the average as a
  hint.
- Rust 239 tests, clippy clean; tsc/eslint/i18n green. Commit `54fddc0`, CI
  `38251392654` (test+web+deploy) green.
- **Live**: the summary exposes all six percentiles and matches a direct SQL
  `percentile_cont` exactly (avg 1660.2, **p50 250.0, p95 4585.2, p99 4927.4**) —
  the mean alone hid a 4.6s p95.
