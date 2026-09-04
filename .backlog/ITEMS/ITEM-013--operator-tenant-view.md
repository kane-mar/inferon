## ITEM-013: Operator View Across Tenants

**As an** Inferon operator
**I want** a cross-tenant spend view
**So that** I can see aggregate platform health and per-tenant cost anomalies (a tenant burning through budget, a shared cache-miss pattern)

**Acceptance Criteria:**
1. A separate operator role/credential (not a tenant key) can list tenants with spend + request volume over a window
2. Tenant keys can never access the operator view, and the operator view never leaks raw tenant keys (only ids + aggregates)
3. Same aggregation shapes as the tenant dashboard (by model / by hash) at platform scope
4. Read-only

**Technical Notes:**
- Contradiction to resolve during refinement: ITEM-007 AC5 ("no tenant can see another tenant's data") stays absolute for tenant keys; operator access needs its own auth mechanism (e.g., an `OPERATOR_API_KEY` env secret or an `is_operator` flag), because the current TenantStore only resolves tenant keys
- Design hint: dashboard SQL already takes `tenant_id`; make the operator path query without the tenant filter but with a `WHERE`-clause guard that only the operator role can omit
