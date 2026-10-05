## ITEM-046: Routing policy & budget controls (dashboard)

**As a** tenant owner
**I want** to configure the heuristic router and the budget circuit breaker from the UI
**So that** cost protection is self-serve rather than an operator-only database edit

The heuristic router (ITEM-015) and the Postgres budget circuit breaker (ITEM-016)
are live in the data plane but have **no UI** — settings live in the DB and are
effectively operator-only. A tenant cannot turn routing on, tune thresholds, or set
their own budget.

**Acceptance Criteria:**
1. A dashboard section to enable/disable the heuristic router and set its thresholds.
2. A section to set the spend budget (monthly) and breaker behavior, with the
   current period's spend vs budget shown live.
3. Changes apply without a redeploy and are reflected in the request path.
4. The current state (routing on/off, budget remaining) is visible at a glance.

**Technical Notes:**
- Writes reuse the existing `tenant_configurations` seam (ITEM-016) — this adds the
  read/write API + UI, not a new store.
- Same page as ITEM-045 (`/dashboard/routing`): one place for "how is traffic handled".
- Audit: who changed a budget/policy and when (owner-only mutations).

**Source:** PO review 2026-10-06 — control surface gaps.
