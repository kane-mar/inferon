## ITEM-099: Latency and quality heatmap

**As a** tenant
**I want** a model × time heatmap of p95 latency and error rate
**So that** degradation is visible at a glance, not inferred from a line chart

**Claim (2026-10-30 brief, landscape).** Langfuse offers "latency heatmaps". Inferon
has time series, p50/p95/p99 and a per-provider table, but no heatmap view.

**Current state (2026-10-30).** `/api/timeseries` returns daily buckets; the
dashboard renders line charts and a provider table. No matrix view.

**Gap.** Presentation only — the data already exists.

**Acceptance Criteria**
1. A model × day (switchable to hour) matrix of p95 latency, with a second mode for
   error rate; the same window switcher as the rest of the dashboard.
2. Empty buckets are visually distinct from zero; colour scaling is documented and
   accessible (not colour-only — values are readable).
3. No new hot-path data and no new aggregate query: build on the existing
   timeseries/provider aggregates; if a new query is needed, it is bounded to the
   selected window.
4. Renders correctly for the empty tenant (onboarding state).

**Technical Notes**
- Keep it a read-only view; do not add a new materialised table for a chart.
- Respect the existing design primitives (`Card`, chart components) so it does not
  become a one-off.

**Ranking note.** Rank **16** of 22 · V1.5/E1 — cheap but the lowest value here: presentation over data that already exists.
---

**Delivered (2026-10-31).** `src/heatmap.rs`: `Cell` (requests, errors, p95) with
`error_rate`, and `pivot` — every model gets one entry per bucket, a missing
bucket is `None` (never `0`), and models are ordered busiest-first, then by name,
so the table is stable between refreshes. `bucket_expr` whitelists `hour`/`day`
(nothing from the query is interpolated).

The report (`/api/performance?days=&step=`) makes **one** bounded aggregate over
the window (`date_trunc` + `percentile_cont(0.95)` grouped by model and bucket)
and pivots it in Rust. The page renders a matrix with the same window switcher;
`?step=hour` / `?step=day` switch the resolution, a dot marks an empty bucket,
and each cell prints `p95 · error%` with a low-contrast tint — values are
readable without relying on colour (AC2). The empty tenant renders nothing
rather than an empty table (AC4).

**Verified**: 5 unit tests (missing ≠ zero, every model gets every bucket,
busiest-first, per-bucket error rate, step whitelist) + the performance
session-scope test updated for the new query shape.
