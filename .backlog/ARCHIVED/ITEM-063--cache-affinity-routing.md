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

## Close-out (2026-10-14)

- `affinity::{keep_warm, is_fresh}`: keep the warm model when its **cache-hit**
  input price is no worse than the chosen model's **uncached** price — grounded
  in the price table, not a guess. TTL 30m bounds a stale pin (failover and
  distribution are never blocked indefinitely).
- `prompt_affinity` (migration 028) + `AffinityStore` seam (Postgres + in-memory
  double), keyed by `(tenant, system_prompt_hash)`.
- The telemetry sink records a warm target (outside the batch transaction,
  best-effort) whenever a trace reported cached prompt tokens.
- The proxy consults it **after** the router/budget override: a warm target that
  beats the downgrade keeps its model and records `traces.cache_affinity`.
- Visibility: summary `cache_affinity_requests`; by-prompt cached tokens, savings
  and `cache_hit_rate`; the dashboard prompt table gains a **Cached %** column.
- Rust **280 → 284** tests, clippy clean; tsc/eslint/i18n green. Commits
  `ebe4b93` + `2372abd`; CI `38491923208` green.
- **Live**: warmed a prefix on `deepseek-v4-pro`, then sent a downgradeable
  request — the router chose `deepseek-flash`, but affinity kept
  `deepseek-v4-pro` (768/806 prompt tokens cached), tracing
  `{reason: cache_affinity, from_model: deepseek-flash, to_model:
  deepseek-v4-pro}`.
- **Live-caught bug**: the bulk `AppState` edit had given `main.rs` the in-memory
  affinity double, so the production lookup always missed (`2372abd`). A second
  reminder that DB-backed paths are not covered by the unit tests.
