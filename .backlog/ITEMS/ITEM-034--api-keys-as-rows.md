## ITEM-034: API keys as rows — multi-key tenants + revoke (Accounts · from ITEM-033)

**As a** tenant who runs several environments (dev/staging/prod)
**I want** separate, individually revocable API keys for the same tenant
**So that** a leaked dev key is revoked without re-seeding the whole tenant

**Acceptance Criteria:**
1. Migration: an `api_keys` table (id, tenant_id FK cascade, label, key_hash
   unique, created_at, revoked_at nullable, last_used_at nullable); every
   existing `tenants.api_key_hash` row is backfilled as the tenant's first key
   (same hash — existing keys keep working), and `api_keys` becomes the auth
   source (the retired `tenants.api_key_hash` column is dropped)
2. A tenant can hold several active keys, each authenticating to the same
   tenant (dashboard scope, budget breaker, schema guard, alerts all key on
   tenant_id — unchanged); a revoked key returns 401 immediately even though
   its hash is known
3. Operator API (still `OPERATOR_API_KEY`-authed at this slice): mint key
   (label, shown once, hash-only stored), list keys, revoke key — no more SQL
   or `seed-tenant.sh` for key operations
4. Regression: existing seeded keys (incl. the ITEM-018 staged key) and the
   full smoke surface (auth 401/403/200, chat, dashboard) pass unchanged

**Technical Notes:**
- Parent: ITEM-033 (archived as superseded) — slice 1 of the accounts horizon
- No dependency; the data-model foundation everything account-y builds on
- Auth seam: `tenant_by_api_key_hash` resolves `api_keys` (active) → tenant in
  one round trip; `seed-tenant.sh` mints into `api_keys` until ITEM-040
- Key prefix convention stays `sk-inferon-`; mint/revoke follow the
  `OPERATOR_API_KEY` gate pattern (ITEM-013/020)
