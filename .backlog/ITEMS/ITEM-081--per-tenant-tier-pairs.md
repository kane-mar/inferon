## ITEM-081: Per-tenant tier pairs (the cheap tier can be your own model)

**As a** tenant with a local or cheap provider
**I want** to declare which cheap model my router may downshift to
**So that** "route the simple stuff to my own box" is a config change, not a
code change — and the router never invents a downgrade I did not choose

**Context (2026-10-28):** the router's downshift pairs are a compile-time
constant (`TIER_PAIRS` in `src/router.rs`: `gpt-4o → gpt-4o-mini`,
`deepseek-v4-pro → deepseek-flash`). A tenant that attaches an on-prem model
(ITEM-051/077) cannot tell the router to prefer it for simple traffic, so the
"cheaper option" only works for the two hardcoded OpenAI/DeepSeek pairs.

**Acceptance Criteria**
1. A tenant owner can declare a **tier pair** — a frontier model and the cheap
   model the router may downshift it to — from `/dashboard/routing`. Pairs are
   per-tenant and additive.
2. Resolution order: the tenant's pairs win for a given frontier model; the
   built-in `TIER_PAIRS` stay as defaults. An empty set means today's behaviour.
3. The pair is only usable when **both** models resolve for that tenant (a
   provider serving each is enabled). A pair whose cheap model stops resolving
   is reported as inactive, never silently ignored.
4. Write-time validation with a typed 400: same model on both sides, unknown
   model, or a pair that would make the downshift non-terminating.
5. Routing still obeys the existing triggers and guardrails — heuristic
   complexity threshold, ITEM-069 weights, budget cap, cache affinity — a pair
   only names the target.
6. The downgrade is traced with the existing `router_event`, now recording the
   pair's **source** (`tenant` / `global`) and the pair itself, so a tenant can
   see why a request was downshifted.
7. A pair pointing at a `local` provider is priced at **$0** (ITEM-077
   semantics) and still attributes the counterfactual — i.e. the downshift shows
   up as savings, not as missing cost.

**Technical Notes**
- Additive JSONB on `tenant_configurations` (migration 0NN), read once in the
  same config fetch the hot path already does; an old hot path ignores it.
- `downgrade_target` becomes a lookup over (tenant pairs ++ global pairs) instead
  of the const alone; keep the matching rule (`frontier` prefix, not already the
  cheap member) unchanged to avoid a starvation loop.
- The dashboard needs the tenant's resolved model list; it is already exposed by
  the providers/models endpoints — no new endpoint required beyond the write.
- Small: one migration, one router lookup, one settings form, tests.

**Source:** 2026-10-28 customer-hosted connectivity review — the "cheaper
option" for a cloud tenant that owns hardware.
