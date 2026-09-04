## ITEM-008: DeepSeek Real Pricing in the Cost Table

**As a** developer using the dashboard
**I want** my DeepSeek spend reported at DeepSeek's real rates
**So that** the cost-leak "aha" shows true dollars, not the default $3/$15 fallback

**Acceptance Criteria:**
1. `deepseek-chat` (and prefix variants) resolves to a real DeepSeek price row instead of the default $3.00/$15.00 row
2. `calculate_cost` uses the row for both streaming and non-streaming traces
3. Prices are rust_decimal literals (never float) matching current DeepSeek API pricing for deepseek-chat
4. Historical traces keep their stored costs (no backfill/migration needed); only new traces change
5. Existing default-row behavior for genuinely unknown models is unchanged

**Technical Notes:**
- Cost table lives in `src/cost.rs::price_for`; add the DeepSeek row before the default arm (prefix-tolerant like gpt-4o)
- Source: ITEM-006 open note — "add a DeepSeek row once pricing is known"
- Live cross-check on the deployment seam: a real DeepSeek request's stored `upstream_cost_usd` should match `tokens × real rates / 1M`
