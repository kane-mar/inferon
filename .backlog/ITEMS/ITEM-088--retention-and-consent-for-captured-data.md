## ITEM-088: Retention and consent for captured payloads

**As a** tenant (and as the operator)
**I want** explicit control over whether and how long raw prompts and completions
are kept
**So that** the distillation flywheel is defensible under a customer's data policy

**Claim (2026-10-30 brief, implicit).** The flywheel depends on storing everything
on the wire. The brief does not address consent, retention or deletion — but
shipping a dataset surface without them is a compliance failure waiting to happen.

**Current state (2026-10-30).** Every trace stores `raw_prompt` (774/774 in the
last 7 days); there is no switch, no retention window, no purge, and no statement
of what is stored.

**Gap.** No tenant-level control, no retention policy, no auditable deletion.

**Acceptance Criteria**
1. A tenant setting controls **capture** of raw payloads (prompt and completion)
   independently of aggregates; turning it off takes effect on the next request
   and the trace still records cost/usage/outcome.
2. A **retention window** per tenant (default 30 days) purges raw payloads by a
   periodic pass; aggregates, costs and outcomes survive.
3. Purge is idempotent and audited (rows affected, window, timestamp), and an
   owner can trigger a purge on demand for a prompt cluster or the whole tenant.
4. When capture is off, ITEM-087's export refuses and the dashboard says why.
5. The guide/wiki states plainly: what is stored, for how long, who can export it,
   and how to turn it off.

**Technical Notes**
- Purge is an `UPDATE … SET raw_prompt = NULL, raw_completion = NULL` in bounded
  batches (no long lock); a partial index on `created_at` where payloads exist.
- Do not delete the trace row: cost/savings/quality history is the product's core.
- The capture switch must be readable on the hot path — it belongs in the same
  config snapshot as ITEM-092.

**Ranking note.** Fourth: the gate that makes ITEM-087 shippable to real tenants.
