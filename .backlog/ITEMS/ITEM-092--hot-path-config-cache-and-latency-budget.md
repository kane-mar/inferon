## ITEM-092: Hot-path config cache and a measured latency budget

**As a** PO making a latency claim
**I want** the proxy's own overhead measured and bounded
**So that** "sub-millisecond wire latency" is either true or withdrawn

**Claim (2026-10-30 brief).** "A single, lightweight binary … adding sub-millisecond
wire latency. It is infrastructure plumbing, not an application-layer framework."

**Current state (measured 2026-10-30, live).** `proxy_ms` is **p50 12 ms / p95
35 ms** for requests under 1,000 prompt tokens (n=5), and ~10 sequential store
reads before dispatch: policy, route/provider, model-enabled, budget, affinity,
fallbacks, guard config. Rust is not the bottleneck; the per-request DB round trips
are. No cache, and no test or benchmark asserts the number.

**Gap.** Overhead is neither bounded nor measured; the claim would not survive
scrutiny.

**Acceptance Criteria**
1. One cached **tenant config snapshot** per request — budget limit/threshold,
   routing fallbacks, tier pairs, policy threshold/weights, guard and capture
   switches — with a short TTL (≤5 s) and explicit invalidation on any write.
   `current_spend_cents` stays authoritative and is read fresh (the breaker must
   never act on a stale counter).
2. Provider/route resolution is cached per `(tenant, model)` for the same TTL, with
   the same invalidation on provider/model changes.
3. A config change is visible within the TTL (test); cache hits/misses and age are
   exposed on the operator metrics endpoint.
4. A benchmark/test asserts the overhead budget: small request `proxy_ms` p50 under
   a stated target on CI hardware, with the measured number reported (not just a
   pass/fail). The target and its hardware are documented.
5. The trace keeps a breakdown (`proxy_ms` plus, at debug level, per-stage timing)
   so a regression names the stage.
6. Behaviour is unchanged when the cache is cold or disabled (kill switch).

**Technical Notes**
- Do not cache spend, in-flight counters or anything that authorises a request.
- Invalidate from the same write paths that update config (dashboard handlers,
  policy publish, provider edits) rather than polling.
- If the target cannot be met on the 1 GB host, say so in the item close-out and
  adjust the claim — that is a valid outcome.

**Ranking note.** Fifth: the cheapest way to make (or honestly withdraw) the
runtime claim.
