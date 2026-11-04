## ITEM-109: Per-step fallback triggers

**As a** tenant
**I want** each fallback step to say which failures it handles
**So that** a chain can send 429s to a cheap retry but 5xxs to a different
model, and the schema guard can join the chain instead of replaying on its own

**Context (2026-11-04, backlog review).** ITEM-045 shipped the ordered fallback
chain and closed with "per-step triggers + guard trigger deferred" (also
recorded in the 2026-10-06 daily log), but no backlog item was captured. Today
every step triggers on the same set — transport, 429 and 5xx — and the guard
keeps its private frontier replay (`fallback_invalid`), so a tenant cannot say
"step 2 only on 5xx" or "the guard's replay is step 3".

**Acceptance Criteria**
1. Each chain step carries a trigger set chosen from transport / 429 / 5xx;
   a step with no explicit set keeps today's behaviour (all retryable classes).
2. Validation refuses an empty trigger set and an unknown class, at save time
   and in the store.
3. The schema guard can be a chain trigger: a guarded request that fails its
   contract advances to the next step instead of the built-in frontier replay
   when the tenant opts in; the existing replay stays the default.
4. `traces.routing_event` records the step index and which trigger fired (the
   reason class already exists; add the step), so the chain's behaviour is
   attributable per request.
5. A chain written before this item behaves exactly as before (no migration
   surprise; the legacy `fallbacks` array still loads).
6. The routing page edits the triggers per step, owner-only, with en/fr copy.
7. Tests: per-class selection, default compatibility, guard-trigger path,
   rejected empty/unknown triggers, an exhausted chain still serves the last
   error.

**Technical Notes**
- `routing_policies.fallbacks` is a JSONB array of model names; widen it to
  `[{model, triggers: [...]}]` and accept the old shape on read (or add a
  sibling column) so no backfill is needed.
- `routing::fallback_reason` already classifies the failure; pass the step's
  trigger set into the decision.
- The guard's replay path is `guard::` + the frontier retry in the proxy — this
  item routes that decision through the same chain instead of adding a second
  mechanism.

**Source:** 2026-11-04 backlog review — deferred scope of ITEM-045 found
untracked (daily/2026-10-06).
