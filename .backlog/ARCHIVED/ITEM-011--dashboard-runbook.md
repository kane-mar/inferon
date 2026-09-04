## ITEM-011: Runbook — Seed Tenants, Dashboard URL, Ops Notes

**As an** operator deploying Inferon
**I want** a runbook covering tenant seeding and the dashboard surface
**So that** I can stand up a working install without reverse-engineering the SQL

**Acceptance Criteria:**
1. README documents: compose up, seeding a tenant (`tenants.api_key_hash` = SHA-256 hex of the developer key, `upstream_openai_key` = the tenant's master key), and the dashboard URL + auth model
2. `.env.example` refreshed to match compose (DATABASE_URL etc.) and clearly note that secrets now live in the tenants table
3. Documented answers: "how do I mint a key for a developer?", "how do I rotate a tenant's master key?", "how do I point the proxy at a different upstream?"
4. A one-command smoke path exists (see ITEM-012) referenced from the README

**Technical Notes:**
- Dashboard is served by the proxy at `GET /v1/dashboard` (HTML) + `/v1/dashboard/api/*` (JSON); all JSON endpoints require `Authorization: Bearer <tenant key>` and are tenant-scoped
- No public docs exist yet; the only usage knowledge lives in commit messages and memory
