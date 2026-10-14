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

## Close-out (2026-10-14)

- `alerts::ErrorSignal`: fires when the 24h error rate strictly exceeds a
  configured fraction, **gated by a minimum request count** so a tiny sample
  cannot fire it (integer/decimal math: `errors > rate × requests`).
- `alerts::LatencySignal`: fires when the 24h p95 exceeds a configured multiple
  of the **preceding 24h** p95 — a per-tenant baseline, so a normal ramp does not
  fire; no baseline means nothing to regress from.
- Both classes run through the existing ITEM-020 lifecycle (start / re-fire ≤24h
  / clear) and appear in the operator alert view.
- Thresholds on `tenant_configurations` (migration 032): `alert_error_rate` 0.5,
  `alert_error_min_requests` 20, `alert_latency_multiplier` 3.0.
- `aggregate_signals` extends its single query with requests/errors/p95 over the
  trailing day vs the preceding day.
- Rust **296 → 298** tests, clippy clean. Commit `989b359`; CI `38644200423`
  (test+web+deploy) green.
- **Live**: after 60 errors / 99 requests (60.6% > 50%, ≥ 20) the engine fired an
  `error_rate` episode — `status: firing`, `detail: {errors_24h: 60,
  requests_24h: 99}`. At 39% (25/64) it correctly stayed silent.
- **AC4** (per-class alerting) is optional and was skipped. **AC5** (per-provider
  degradation) is split to **ITEM-071**.
