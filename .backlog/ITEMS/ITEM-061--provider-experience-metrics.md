## ITEM-061: Provider experience metrics

**As a** tenant choosing or rotating providers
**I want** per-provider latency and error rate, not just cost and a reachability
badge
**So that** I can pick and rotate providers on evidence, and see a degrading
provider before it costs me

`by-provider` shows **spend**; the provider row shows a persisted
reachable/unreachable badge. Neither shows how a provider actually behaves under
load — its TTFT, p95, or error rate — so a provider that is cheap but slow, or
failing intermittently, is invisible.

**Acceptance Criteria:**
1. Per-provider **latency** (mean + p95 of TTFT/total, reusing ITEM-056) and
   **error rate** (reusing ITEM-057 classes) are exposed.
2. The Providers page shows these alongside the existing reachability + spend.
3. A provider whose error rate or p95 degrades past a threshold raises an alert.
4. The data is available per window, so a provider's trend (not just a snapshot)
   is visible.

**Technical Notes:**
- Provider attribution already exists (`traces.provider_id`); this is aggregation
  over the same rows as ITEM-056/057, keyed by provider.
- Reuse the reachability probe's stored result for the badge; do not conflate
  "reachable" with "healthy".

**Source:** 2026-10-12 metrics review — provider behaviour is unmeasured; only
its reachability and its bill are visible.
