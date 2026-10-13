## ITEM-068: Error-rate alerting (ITEM-057 AC4)

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

**Technical Notes:**
- Extend `AlertThresholds` + the `aggregate_signals` query (requests/errors in
  the window) and add a pure `ErrorSignal::is_anomalous` to `alerts.rs`, mirroring
  `SpendSignal`/`WasteSignal` so the rule is unit-testable without a DB.
- `traces.error_class` (ITEM-057) makes the per-class signal derivable.

**Source:** 2026-10-13 — ITEM-057 AC4 split out.
