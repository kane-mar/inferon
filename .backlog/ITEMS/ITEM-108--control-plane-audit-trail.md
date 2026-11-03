## ITEM-108: Audit trail for control-plane changes

**As a** tenant owner (and as an operator)
**I want** to see who changed a budget, guard, router, fallback, tier pair or a
capture/lint/PII setting and when
**So that** a change can be attributed and reversed instead of being an
anonymous edit

**Context (2026-11-04, backlog review).** ITEM-046 shipped the routing and
budget controls and closed with "Audit trail deferred" — its technical notes
asked for "who changed a budget/policy and when (owner-only mutations)". The
daily log of 2026-10-06 records the deferral, but no backlog item was captured.
The gap is real: `tenant_configurations` and `routing_policies` carry
`updated_at` and nothing about the actor, so a two-owner tenant cannot tell who
raised the cap or turned the guard off. `tenant_policies` (ITEM-064) is
append-only and versioned, but that is the adaptive loop's own history, not the
config writes.

**Acceptance Criteria**
1. Every owner-only config mutation records an append-only event: tenant, actor
   user id, setting name, old/new value (non-secret only), and timestamp.
2. The events cover the settings the dashboard can change: budget cap and cheap
   fallback, guard, router on/off and threshold, fallback chain, tier pairs,
   capture/retention, lint, PII modes and pruning policy.
3. `/dashboard/routing` shows the most recent changes, owner-only, with actor
   and time; the log cannot be edited or deleted from the UI.
4. Secrets never enter the log: provider credentials, webhook URLs and API keys
   are recorded as "changed", never by value.
5. A config write and its event are one transaction — a failed write leaves no
   event, and an event never exists for a write that did not apply.
6. Migration is additive and does not backfill a fake actor for existing rows;
   the page says "before the audit trail" for older state.
7. Tests: store round-trip, event on every mutation, no event on a rejected
   write, owner can read, member cannot.

**Technical Notes**
- One `config_audit` table keyed by `(tenant_id, at)` with a JSONB before/after
  pair, or per-field rows if that reads better; the dashboard only needs the
  last N.
- The actor is the session user id (ITEM-035) — the config endpoints already
  resolve it for owner checks.
- Keep this presentation-free at the store seam; the routing page renders it.

**Source:** 2026-11-04 backlog review — deferred scope of ITEM-046 found
untracked (daily/2026-10-06).
