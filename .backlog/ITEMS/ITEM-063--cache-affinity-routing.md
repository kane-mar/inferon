## ITEM-063: Cache-affinity routing

**As a** tenant paying per token
**I want** Inferon to preserve and exploit provider-side prompt caching
**So that** repeated prefixes (the 10 repeated system prompts) keep hitting the
cache instead of being re-billed at full price

Real traffic already shows ~90% cache hit on ~520k-token prompts. Provider caches
are **prefix-based and per-provider/per-account**, so a routing decision that
switches provider or model can **destroy** the cache. The current router picks the
cheap tier by complexity with no knowledge of whether that target holds the
prefix.

**Acceptance Criteria:**
1. Requests sharing a `system_prompt_hash` are routed with **affinity** to the
   provider/model that has that prefix warm, unless a strong reason overrides.
2. The router weighs the **expected cache-hit value** (using observed
   `cached_prompt_tokens` for that group) against the downgrade value, instead of
   trading a warm cache for a marginally cheaper miss.
3. Cache-hit rate is reported **per affinity group / prompt cluster**, and the
   system quantifies how much affinity preserved or created.
4. Affinity is **bounded (TTL)** so a stale pin cannot block failover or healthy
   load distribution.
5. The decision is observable: a `cache_affinity` event/field on the trace, and
   the dashboard shows cache hit rate by prompt cluster.

**Technical Notes:**
- `system_prompt_hash` and `cached_prompt_tokens` already exist — this is
  aggregation + a routing input, not new telemetry.
- Cache TTLs differ by provider (DeepSeek's prefix cache is minutes–hours);
  store the observed last-hit time per (prefix, provider, model) to age affinity.
- Interaction with ITEM-058: cache savings are already computed; affinity should
  *maximise* them.
- Interaction with ITEM-054: a DeepSeek downgrade may be cache-safe only if the
  cheap tier shares the cache domain — verify before pinning.

**Source:** 2026-10-12 metrics review / adaptive-operation analysis — the
cache-hit rate is the largest cost lever and routing currently ignores it.
