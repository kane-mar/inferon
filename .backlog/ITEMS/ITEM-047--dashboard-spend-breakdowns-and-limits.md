## ITEM-047: Dashboard — provider/model/key spend and spend controls

**As a** tenant owner
**I want** to see spend broken down the way I think about it, and cap it
**So that** the dashboard answers "who/what is costing me money, and how do I stop it"

The 2026-10-05 redesign added time-series charts, KPIs (cost/request, error rate,
TTFT), and period comparison. What is still missing is the *controllable* view: the
breakdowns that map to the things a tenant can act on, and limits.

**Acceptance Criteria:**
1. Spend/usage breakdown by **provider**, **model**, **API key**, and **system
   prompt**, each over the selected window and comparable to the prior period.
2. Per-key and per-provider spend **limits** with alerting (reuse ITEM-020 alerts).
3. Cost anomalies surfaced inline (which model/key spiked) rather than only in alerts.
4. A clear empty/onboarding state for a brand-new tenant (what to do first).

**Technical Notes:**
- Extends the existing dashboard JSON APIs (summary/timeseries/by-model/by-prompt)
  with by-provider and by-key dimensions (both already exist as join keys:
  `providers`, `api_keys`).
- Depends on ITEM-043/044 (provider + model concepts) for the provider/model views.
- The chart/metric primitives from the redesign (`dashboardMetrics.ts`, `LineChart`)
  are reused — this is new queries + panels, not a new chart stack.

**Source:** PO review 2026-10-06 — "the dashboard doesn't show anything useful".
