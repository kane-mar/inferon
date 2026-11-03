## ITEM-102: Dashboard surfaces for pruning, evals and the prompt registry

**As a** tenant owner
**I want** to use the features we shipped without curl
**So that** they are product features, not internal capabilities

**Context (2026-10-31).** ITEM-091 (pruning), 098 (evals) and 097 (prompt
registry) were delivered and tested, but each was reachable only through its JSON
API. A tenant with a browser could not turn pruning on, pin a gold set, or name a
prompt.

**Acceptance Criteria**
1. Pruning is configurable from the dashboard: enable, drop-duplicate-turns, an
   optional token ceiling and the holdout — owner-only, saved as a new versioned
   policy.
2. Evals are usable from the dashboard: pin a gold set from qualified examples,
   start a run against a model, and compare two runs to get the gate's verdict.
3. The prompt registry is usable from the dashboard: name a content hash, see
   each version's requests/ok-rate/spend/thumbs, and diff two versions.
4. Empty and non-owner states are honest: no data says so, a member sees the
   read model but not the controls, and a tenant with capture off is told why a
   diff is unavailable.
5. No new backend and no second read model: the pages call the endpoints those
   items already shipped.

**Delivered (2026-11-03).** `PrunePanel` on `/dashboard/routing`; new tabs
`/dashboard/evals` (`EvalsPanel`) and `/dashboard/prompts` (`PromptsPanel`),
added to the dashboard's tab bar. One `EvalsPage` / `PromptsPage` message
namespace each, all en+fr.

**A real gap found while writing the auth tests:** `POST /api/prune` existed as a
handler but had **never been added to the router** — the opt-in endpoint answered
404 in production, so ITEM-091's "a tenant can opt in from the dashboard" was not
actually true. Fixed here, with the test that caught it
(`only_an_owner_can_publish_a_pruning_policy`).

**Verified live (tenant kane-mar):**
- Tabs render and the active tab tracks the page; `evals` and `prompts` both 200.
- Pinned a gold set from qualified traffic (1 case — one distinct prompt), ran
  `deepseek-flash` (1/1 scored, 100 %, $0.000129), run listed as `complete`.
- Labelled a real prompt hash: `invoice-extraction`, 6 requests, 33 % ok,
  $0.000344 — rendered in the registry.
- Pruning round-trip: `enabled: true` → v40 → visible in `/api/config`;
  `enabled: false` → v41. Left **off** (its evidence gate is ITEM-090 findings).
- Temp API key minted for the traffic and revoked afterwards.

**Tests:** 506 Rust (2 new: session-scoped read models, owner-only prune),
`tsc`/eslint/i18n clean.
