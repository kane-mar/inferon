## ITEM-053: Cache-hit telemetry & savings

**As a** tenant owner
**I want** to see how much of my prompt spend is served from the provider's
prompt cache, and how much that saves
**So that** the single biggest cost lever in real traffic is visible and
actionable

Real usage (ITEM-018 close-out, 2026-10-12): 641.5M prompt tokens on
`deepseek-flash` cost **$14.59**; at the full miss price that would be **~$141**
— a ~90% cache hit and **~$126 saved** — but the dashboard shows only total
spend. The cost engine is already cache-aware (ITEM-017); the cached-token count
is parsed into the trace but **not persisted**.

**Acceptance Criteria:**
1. The cached prompt-token count is persisted per trace (`traces.cached_prompt_tokens`).
2. The dashboard shows cache **hit rate** and estimated **savings** (hit price vs
   miss price) for the selected window, per model/provider.
3. A tenant can see which prompts/models are **not** cached (cache-miss spend).
4. The numbers reconcile with `upstream_cost_usd` (savings = miss-priced cost −
   actual cost).

**Technical Notes:**
- `TraceMessage.cached_prompt_tokens` already exists — add the sink column + the
  dashboard aggregation.
- Reuse `cost::price_for` (miss vs hit rates) and the ITEM-047 breakdown panels.

**Source:** ITEM-018 AC5 finding, 2026-10-12 real traffic.
