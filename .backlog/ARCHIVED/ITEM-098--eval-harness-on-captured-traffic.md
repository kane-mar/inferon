## ITEM-098: Eval harness on captured traffic

**As a** tenant choosing between a frontier model and a cheap/distilled one
**I want** to score both against a pinned gold set from my own traffic
**So that** the routing and distillation decisions are evidence-based

**Claim (2026-10-30 brief, landscape).** Langfuse provides "async telemetry & eval";
the brief's flywheel implies a quality gate before an adapter is trusted. Inferon
has outcomes and thumbs but no gold set, no eval run and no comparison.

**Current state (2026-10-30).** `outcome` classification, thumbs, guard results, and
`Trainer`/`Gate`/`Promoter` seams (ITEM-076) — but `Gate` has no implementation and
there is no way to run a model comparison over captured inputs.

**Gap.** No pinned dataset, no eval execution, no regression gate.

**Acceptance Criteria**
1. A **gold set** can be built from ITEM-086's verified examples and pinned
   (immutable, versioned, with provenance: prompt hash, model, trace id).
2. Running an eval sends the pinned inputs to a chosen model/provider and scores:
   schema-valid (when a contract exists), exact match (when a reference exists),
   outcome, and optional human thumbs; cost and latency are recorded per case.
3. A run is owner-initiated, bounded (case count and spend), traced, and stored with
   its configuration so it is reproducible.
4. A **regression gate**: compare two runs (model A vs B, or adapter v1 vs v2) and
   return a verdict with the delta per case class; ITEM-076's `Gate` consumes it.
5. A failed/partial run is reported as such — never silently scored as zero.
6. No eval traffic is billed to the tenant's budget unless explicitly opted in.

**Technical Notes**
- Eval calls are real provider calls; route them through the same upstream path but
  mark them (e.g. `traffic_kind = eval`) so they never pollute the production
  aggregates used for routing/policy.
- Exact-match scoring needs a reference; when there is none, use schema validity plus
  the guard/outcome signals and say so.

**Depends on** ITEM-086; feeds ITEM-076.
**Ranking note.** Rank **13** of 22 · V3/E3 — needed before any adapter is promoted, but its consumer (076) is gated on Slice 3 infra.
---

**Delivered (2026-10-31).** `src/eval.rs` (pure: `normalize`, `score` →
exact / schema / outcome, `compare` → insufficient / unchanged / improved /
regressed with a 20-case floor and a 5-point margin, `answer_text` for both
surfaces) and `src/eval/runner.rs` (pin, load, execute, finish).

**Pin** copies the exact `raw_prompt` and the reference `raw_completion` out of
`traces` into `eval_gold_cases`, one case per distinct prompt hash, and versions
the set per tenant — so later retention, pruning or redaction cannot change what
a run measured.

**Run** is owner-initiated and bounded twice (`max_cases`, `max_cost_cents`);
each case is a real provider call with the pinned input, the model substituted,
streaming off. Scoring never uses a judge model: schema validity when the request
carried a contract, normalised exact match against the reference when one exists,
otherwise the outcome signal — and the method is stored per case, so two runs are
never compared across different measurements. A case that could not even be sent
is `failed` with a reason; a run with failures or a hit bound is `partial`, never
padded with zeros (AC5).

**Billing**: eval calls are recorded only in `eval_*` tables, never `traces`, so
they cannot reach the budget, the aggregates, the router or the policy — AC6 is
satisfied by construction rather than by a filter.

**Not built**: a dashboard UI (the API is the surface), and the ITEM-076 `Gate`
adapter — `compare` returns the verdict the promoter will consume, but the
promoter itself is still gated on Slice 3 infra.

**Verified**: 14 unit tests (scoring precedence, margins, partial runs, both
answer shapes). The DB orchestration is exercised live after the next deploy.
