## ITEM-006: Cost Linter & Cost Calculator

**As a** developer
**I want** automatic detection of cost anti-patterns and per-request cost accounting on my traffic
**So that** I see where money leaks before I change anything

**Acceptance Criteria:**
1. Waste rule A: request model contains `gpt-4o` with prompt_tokens < 100 and completion_tokens < 50 → `waste_detected.over_provisioned = true`, `suggested_model = gpt-4o-mini`
2. Waste rule B: repeating 4,000+ token system prompts without provider cache headers are flagged
3. Waste rule C: gpt-4o used for simple unstructured text is flagged
4. `upstream_cost_usd` is computed per 1M tokens from the price table: gpt-4o $2.50/$10.00, gpt-4o-mini $0.15/$0.60, default $3.00/$15.00 (prompt/completion), stored as NUMERIC(10, 6)
5. `waste_detected` JSONB is persisted on the trace (ITEM-005 flush)
6. Cost math uses rust_decimal — never float

**Technical Notes:**
- Source: implementation spec linter + cost table (see wiki `aio-proxy-overview.md` → Implementation Latency & Waste Rules)
- **Open**: DeepSeek is the default model (project decision) but no DeepSeek pricing exists in any source — add a DeepSeek row to the price table once pricing is known; until then default row covers it
- Linter runs in the background batch worker (async, off hot path)
