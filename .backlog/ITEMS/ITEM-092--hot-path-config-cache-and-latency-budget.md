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

**Ranking note.** Rank **5** of 22 · V4/E2.5 — makes the runtime claim honest (or withdraws it) and gives 090 the prompt cache it needs.
## Progress (2026-10-30) — the contract and its enforcement

The claim is now measurable and enforced, not aspirational:

**Contract.** For a warm request with a small body, the proxy's own overhead —
request start to just before the first upstream dispatch, i.e. `traces.proxy_us`
— is **p50 < 1,000 µs** and **p95 < 2,000 µs**. It excludes the upstream and the
response body; it includes authentication, config, routing, the budget/router
decision and request parsing.

**What was built**
- `traces.proxy_us` (migration 037) alongside `proxy_ms`, so a sub-millisecond
  claim is expressible at all (ms integers cannot express it).
- `src/hotpath.rs`: a per-tenant read cache — controls/policy/fallbacks (5 s),
  spend (1 s, already ≤500 ms stale from the telemetry flush), resolved route +
  disabled flag (5 s), warm affinity target (30 s). Writes invalidate the
  tenant snapshot from every dashboard/operator path; the TTL is the backstop for
  out-of-band changes.
- **Authentication is cached too**, for 1 s: the first warm-path measurement with
  only a config/route cache showed p50 1,382 µs (p95 5,992 µs) on the 1 vCPU host
  — the auth query alone was 1–4 ms under contention. The key→tenant entry is
  dropped by `invalidate_key`/`invalidate` from every revoke path, so a revoked
  key stops on the next request in this process; a second instance would lag by
  at most the 1 s TTL. This is the one place where the cache touches
  authentication and it is deliberately short.

**The suite**
- `proxy_overhead_is_sub_millisecond_for_small_requests` (src/tests.rs): 60
  warmed requests through the router, p50/p95 of `proxy_us` against the contract.
- `a_warm_request_does_not_read_tenant_config_per_request`: a counting store
  proves a 20-request burst causes at most one budget read (the spend TTL), not
  one per request.
- `a_trace_records_its_proxy_overhead` now asserts the µs and ms values agree.
- `the_config_cache_is_dropped_when_the_tenant_writes` and
  `a_provider_change_reaches_the_cached_route_only_after_invalidation`: the cache
  cannot outlive a write.
- `scripts/perf-check.sh` runs **in the deploy job, on the host, after the health
  check**: it seeds a throwaway tenant with a bogus upstream key (so requests 401
  and cost nothing), sends 40 warmed requests, reads p50/p95 from `proxy_us`, and
  fails the deployment run if either budget is exceeded. It prints the measured
  numbers to the CI log every time.

**Not yet closed**: the live numbers still have to land on a deploy; the item
stays open until `perf-check.sh` is green on the durable host.
