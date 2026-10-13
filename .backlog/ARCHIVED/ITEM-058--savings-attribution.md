## ITEM-058: Savings attribution (routing, fallback, cache, frontier delta)

**As a** tenant paying the bill
**I want** to see how much Inferon actually saved me, and by which mechanism
**So that** the value is provable, not just a lower number

Inferon already traces the **events** that cause savings — `router_event`
(downgrade), `routing_event` (fallback), `cache_savings_usd` — but never computes
what those events **saved**. The slice-2 promise ("route cheap when safe") has no
attribution.

**Acceptance Criteria:**
1. For each routed/downgraded/fallback request, record the **counterfactual**:
   the model that *would* have served it and the cost delta. Router saving ≈
   `cost(from_model) - cost(to_model)`.
2. The summary exposes a **savings breakdown**: `router_savings`,
   `fallback_savings`, `cache_savings`, and **cache-miss spend** (what was paid
   vs what a full-miss would cost).
3. Per-model "cost if frontier" is available, so a tenant sees the value of the
   cheap tier.
4. Totals reconcile: `actual spend + attributed savings = unreduced spend`.
5. Historical rows without an event contribute zero savings (metric accrues from
   deploy forward), stated plainly in the UI.

**Technical Notes:**
- The counterfactual for a router event is the `from_model` already in
  `router_event`; for fallback it is the requested model in `routing_event`.
  Cross-cost with the `cost::lookup` prices already in-process.
- Store the delta on the trace (`savings_usd` + `savings_kind`) or compute on
  query from the stored events — prefer storing at write time so prices can't
  retroactively rewrite history.
- ITEM-053 already stores `cache_savings_usd`; this item unifies the savings view.

**Source:** 2026-10-12 metrics review — events are traced but their value is not
computed.

## Close-out (2026-10-13)

- `cost::savings_vs(counterfactual, served, tokens)` = `max(0, asked - served)`
  (unit-tested against the price table).
- The telemetry worker computes each trace's saving from its `router_event` /
  `routing_event` `from_model` versus the model that served, and records
  `traces.savings_usd` + `traces.savings_kind` (`router` | `fallback`)
  (migration 027).
- `/api/summary` exposes `router_savings_usd`, `fallback_savings_usd`,
  `total_savings_usd` (incl. cache) and `unreduced_cost_usd`; it **reconciles**:
  `actual + router/fallback savings = unreduced`.
- `/api/by-model` exposes `savings_usd` + `frontier_cost_usd` per model (the
  value of the cheaper tier).
- Web: the dashboard savings KPI is now **total savings**, with cache savings as
  a hint.
- Rust **275 → 277** tests, clippy clean; tsc/eslint/i18n green. Commit
  `4b4b61c`; CI `38408441964` (test+web+deploy) green.
- **Live**: a simple `deepseek-v4-pro` request was router-downgraded to
  `deepseek-flash` → `savings_usd = 0.000082` (`kind = router`) on an actual cost
  of `0.000041`; the summary reported `total_savings_usd = 0.000252`
  (router 0.000082 + cache 0.000170) and `unreduced_cost_usd = 0.000708`
  (`0.000626 + 0.000082` — the reconciliation holds). Historical rows contribute
  zero (no event → no savings).
