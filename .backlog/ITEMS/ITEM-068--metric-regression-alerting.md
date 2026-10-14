## ITEM-068: Metric-regression alerting (error rate, provider health, latency)

**As a** tenant
**I want** to be alerted when my error rate (overall or for a specific class)
crosses a threshold
**So that** a provider outage or a broken client surfaces without me watching
the dashboard

Split from ITEM-057 (2026-10-13): the taxonomy and its visibility are done
(`traces.error_class`, `errors_by_class` on the summary/quality views); this item
is the alerting half.

**Acceptance Criteria:**
1. A new alert class (e.g. `error_rate`) fires when a tenant's error rate over a
   rolling window exceeds a configured fraction, with a minimum request count so
   a tiny sample cannot fire it.
2. The class is delivered through the existing ITEM-020 lifecycle (start /
   re-fire ≤24h / clear) and the operator alert view.
3. Thresholds live on `tenant_configurations` (defaults conservative) and are
   tunable, consistent with the spend/waste thresholds.
4. Optional: per-class alerting (e.g. only `transport`) is expressible, not just
   the overall rate.
5. **Per-provider degradation** (folded in from ITEM-061 AC3, 2026-10-14): a
   provider whose error rate or p95 latency crosses a threshold raises an alert,
   so a cheap-but-failing or slow provider surfaces before it costs the tenant.
6. **Latency regression** (folded in from ITEM-066 AC3, 2026-10-14): a p95
   regression of TTFT, total duration, or proxy overhead (ITEM-056/066) over a
   window raises an alert, with a per-tenant baseline so seasonality does not
   fire on a normal ramp.

**Technical Notes:**
- Extend `AlertThresholds` + the `aggregate_signals` query (requests/errors in
  the window) and add a pure `ErrorSignal::is_anomalous` to `alerts.rs`, mirroring
  `SpendSignal`/`WasteSignal` so the rule is unit-testable without a DB.
- `traces.error_class` (ITEM-057) makes the per-class signal derivable.

**Source:** 2026-10-13 — ITEM-057 AC4 split out; extended with ITEM-061 AC3
(per-provider degradation) and ITEM-066 AC3 (latency regression) on 2026-10-14.
