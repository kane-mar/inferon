## ITEM-100: Multi-provider load balancing

**As a** tenant with the same model on two providers
**I want** requests spread across them
**So that** one provider's rate limit or latency spike does not throttle all traffic

**Claim (2026-10-30 brief, landscape).** LiteLLM does "simple load balancing and
fallbacks". Inferon resolves to the highest health weight (ITEM-069) or the default
provider and only moves on a retryable failure.

**Current state (2026-10-30).** `resolve_route` picks the max-weight provider among
those advertising the model; the fallback chain is ordered. Two healthy providers do
not share load — the second is only used when the first fails.

**Gap.** No distribution, no in-flight awareness.

**Acceptance Criteria**
1. When several providers serve a model, selection distributes across them
   (weighted random or round-robin — stated at pull) instead of always the maximum;
   health weights still demote a failing provider and can zero it out.
2. Selection is deterministic under a seed for tests, and per-provider in-flight
   counts are visible on the operator metrics.
3. A provider at its configured limit (ITEM-095) is skipped rather than throttled.
4. The fallback chain is unchanged; a single-provider model behaves exactly as today.
5. Traces record which provider served each attempt (already there) and the selection
   reason (`weight | round_robin | only`).

**Technical Notes**
- Keep the choice a pure function of `(candidates, weights, in-flight, seed)` so it
  is unit-testable.
- Do not add a background balancer; the decision is per request, from cached state
  (ITEM-092).

**Ranking note.** Sixteenth: real but commodity; health weighting already covers the
failure case.
