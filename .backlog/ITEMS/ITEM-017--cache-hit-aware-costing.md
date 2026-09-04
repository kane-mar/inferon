## ITEM-017: Cache-Hit-Aware Cost Accounting

**As a** developer on the dashboard
**I want** prompt cost to reflect provider cache hits
**So that** my DeepSeek spend is reported truly — not overstated up to ~30× for repeated prompts (DeepSeek auto-caches long prefixes)

**Acceptance Criteria:**
1. `calculate_cost` splits prompt tokens: `(prompt − cached) × miss_rate/1M + cached × hit_rate/1M + completion × rate/1M` when the model has a hit rate
2. DeepSeek rows carry hit rates from the pricing page oracle (off-peak, matching ITEM-008's estimate convention): v4-flash family cache-hit input $0.007, v4-pro $0.022
3. Models without a hit rate (gpt-4o tier, default row) price exactly as today — no behavior change
4. Historical traces keep their stored costs (no backfill); only new traces use the cache split
5. `cached_prompt_tokens` comes from the already-parsed `usage.prompt_tokens_details.cached_tokens` stored per trace (ITEM-005) — flows into the worker's cost call unchanged end to end

**Technical Notes:**
- Signature change: `calculate_cost(model, prompt, completion)` gains the cached-token count; call sites: the telemetry worker enrichment and the unit tests
- Oracle: DeepSeek pricing page `api-docs.deepseek.com/quick_start/pricing` — v4-flash cache-hit input $0.007 off-peak (vs $0.22 miss), v4-pro $0.022 (vs $0.66); peak is 2×
- Source: flagged as a follow-up to ITEM-008 (single miss-rate row was a documented approximation)
- Relevant to ITEM-016 (budget breaker) later — the spend counter inherits this accuracy
