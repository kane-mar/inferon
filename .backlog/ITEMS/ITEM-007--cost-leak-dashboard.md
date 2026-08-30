## ITEM-007: Cost-Leak Dashboard

**As a** developer
**I want** to see where my spend goes the moment I switch my baseURL
**So that** I get the "aha" moment — e.g. "$340 this week on 22,000 JSON extractions that could run on an 8B model for $7"

**Acceptance Criteria:**
1. Dashboard shows spend by model, by tenant, and by system_prompt_hash cluster over a selectable time window
2. Waste flags surface with estimated $ impact: over-provisioned calls, cache-miss system prompts, model misuse
3. Reads from Postgres `traces` via aggregate queries (no new write path)
4. A typical month of MVP-scale traces loads within 2s
5. Read-only; no tenant can see another tenant's data

**Technical Notes:**
- Slice 1 user-facing deliverable (see wiki `inferon-product-roadmap.md` → Slice 1)
- Larger item — keep at the bottom of the backlog; break down into charts/aggregations/tenant-isolation sub-items when it reaches the top
- Served by the proxy container or a separate lightweight web container (compose profile)
