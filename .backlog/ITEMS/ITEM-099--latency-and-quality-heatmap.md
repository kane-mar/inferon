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

**Ranking note.** Fifteenth: cheap, visible, but pure parity.
