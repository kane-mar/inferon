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
