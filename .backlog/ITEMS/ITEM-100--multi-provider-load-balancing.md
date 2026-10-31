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

**Ranking note.** Rank **14** of 22 · V2/E2 — health weights already cover the failure case; distribution is convenience.
---

**Delivered (2026-10-31).** `src/loadbalance.rs`: `choose(candidates, seed,
strategy)` — eligible candidates are those with a positive, finite weight (so a
policy-zeroed provider is skipped and an unreachable one is filtered before the
call), one candidate returns `Reason::Only` (the old behaviour), otherwise
weighted or round-robin. Weights are expanded to thousandths so the choice is
integer maths, and `mix` (splitmix64) spreads the per-request counter — a bare
increment would sit inside the first weight bucket for a thousand requests.

Wiring: `resolve_route` asks `balance_across_providers` first; it resolves each
provider, drops the ones whose reachability probe failed, builds the weights
from the active policy and picks. `Route.selection` is recorded on
`traces.provider_selection`, and `metrics::provider_started` holds a per-provider
in-flight gauge exposed on the operator metrics.

**Verified**: 10 unit tests (single/zeroed/round-robin/weighted distribution,
determinism, the mix, shares) + the updated route test: equal weights use both
providers, a 0.1 vs 1.0 weight still serves both but the healthy one takes >80%,
and a 0.0 weight is never chosen.
