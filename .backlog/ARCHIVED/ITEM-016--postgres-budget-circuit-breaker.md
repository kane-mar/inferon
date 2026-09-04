## ITEM-016: Postgres Budget Circuit Breaker (Slice 2)

**As a** tenant
**I want** my spend capped at my monthly budget
**So that** a runaway integration can't blow through real money

**Acceptance Criteria:**
1. Budget counter lives on `tenant_configurations` (`current_spend_usd` in integer cents — atomic `UPDATE ... SET current_spend_usd = current_spend_usd + $cents` on the routing path)
2. Before forwarding, the proxy checks spend vs the tenant's monthly limit: over the hard cap → HTTP 429 with an OpenAI-style error body
3. Within 15% of the cap → `EnforceCheapFallback`: the model parameter is rewritten to the cheapest configured model
4. Counter updates derive from the trace cost path (same Decimal math as ITEM-006) — never float
5. Durable: no Redis fast-path in the MVP (documented scale-out later)

**Technical Notes:**
- Source: engineering spec Slice 2 BudgetCircuitBreaker, superseded by the project-direction decision (Postgres counter in integer cents; Redis fast-path + async reconciliation is the scale-out path — see `wiki/llm-inference/inferon-project-direction.md`)
- Requires a `tenant_configurations` migration (001_init.sql is append-only idempotent; add 002)
- This item supersedes the wiki's old Disputed Redis counter format block
