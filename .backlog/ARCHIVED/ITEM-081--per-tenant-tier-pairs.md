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

## Close-out (2026-10-29)

- **Router seam** (`src/router.rs`): `TierPair { frontier, cheap }`,
  `PairSource { Tenant, Global }`, `downgrade_for` (the tenant's pair wins, the
  built-in table is the fallback) and `maybe_downgrade_with_pairs` (AC1/AC2).
  `maybe_downgrade_with` delegates with no pairs, so the linter and the
  platform-key path are unchanged. `validate_pairs` rejects an empty side, the
  same model twice (AC4), a model the tenant cannot route to (AC4), a duplicate
  frontier, and a loop — including a loop closed *through* the built-in table
  (`deepseek-flash → deepseek-v4-pro` would cycle with the default pair).
- **Hot path** (`src/budget.rs`): the pairs live on `tenant_configurations`
  (`tier_pairs` JSONB, migration 036) and ride on the row `budget_for` already
  loads — no extra query per request. A tenant pair is applied only while its
  cheap model still resolves (`pair_is_routable`, AC3); if the model was
  disabled or the provider removed after the pair was saved, the client's model
  stands. Controls saves (`update_config`) preserve the pairs.
- **Trace** (AC6): `router_event` now carries `pair_source: "tenant" | "global"`
  alongside `from_model`/`to_model`/`reason`; savings attribution is unchanged
  (`savings_kind: router`).
- **Dashboard** (AC1/AC3): `GET /api/routing` returns
  `tier_pairs: [{frontier, cheap, active}]`, `active` false when either side no
  longer resolves — reported, never dropped. `POST /api/routing/tier-pairs`
  (owner-only, max 8) trims, validates and persists, with typed 400s:
  `tier_pair_empty|same_model|unknown_model|duplicate|cycle|too_many`.
- **Edge**: `GET /v1/edge/config` syncs `budget.tier_pairs`, so a node applies
  the same pairs (a pair that targets the local model is the point of AC7).
- **Web**: "Tier pairs" card on `/dashboard/routing` — list with an inactive
  badge, frontier/cheap selects fed by the tenant's enabled models, add/remove
  through the new endpoint. Copy in `en` + `fr`.
- **Local**: Rust **323 → 337** tests (tenant override + global fallback;
  same/unknown/duplicate/cycle validation; hot path rewrites and traces
  `pair_source`; pair stops applying when its cheap model disappears; the
  config row survives a controls save; owner/member and active-flag handler
  tests), clippy clean, web `tsc`/eslint/i18n/vitest green.
- **Live (2026-10-29)**, deploy `5223ead`, CI run `40854155133`:
  migration 036 present (`tier_pairs jsonb default '[]'`); `GET /api/routing`
  200 `{"fallbacks":[],"role":"owner","tier_pairs":[]}`; `POST` a valid pair →
  200 `active:true`; same model → 400 `tier_pair_same_model`; unadvertised
  `deepseek-chat` on a DeepSeek-only tenant → 400 `tier_pair_unknown_model`;
  `deepseek-flash → deepseek-v4-pro` → 400 `tier_pair_cycle`; clearing → 200.
  Hot path on the platform-key tenant: pair `deepseek-v4-pro → deepseek-chat`,
  a live simple request was served by **deepseek-chat** (not the global
  `deepseek-flash`) with `router_event {"from_model":"deepseek-v4-pro",
  "to_model":"deepseek-chat","reason":"heuristic_router",
  "pair_source":"tenant"}`, `savings_kind=router`, `savings_usd=0.000026`;
  the pair was cleared afterwards.
