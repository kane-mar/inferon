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

## Verification record (2026-09-06)

- **AC1 (migration 006, schema seam)**: verified on real Postgres 16 on the
  durable host with a throwaway container (never the live compose project):
  fresh-volume shape (api_keys columns/indexes, FK cascade, `key_hash` unique,
  `tenants.api_key_hash` retired), the pre-006 upgrade path (re-added column +
  legacy tenant → backfill as a `migrated` api_keys row → column retired), and
  idempotent re-apply of 001..006 (no duplicate backfill). Applied live: the
  DB's 2 real tenants were backfilled (label `migrated`, not revoked) and
  auth now resolves through api_keys.
- **AC2 (revoke → immediate 401)**: unit (revoked hash resolves to nothing;
  sibling keys unaffected; idempotent revoke) + live smoke arm: mint → chat
  502 (auth passed) → revoke via the operator API → same raw key 401.
- **AC3 (operator key API)**: `GET/POST /v1/dashboard/api/operator/keys`,
  `POST .../{key_id}/revoke` behind the OPERATOR_API_KEY gate (401 no key /
  403 tenant key / 200 operator). Mint returns the raw key once; list is
  metadata-only (asserted: no `sk-inferon-` material). Live smoke arms all
  passed; also verified live against the real operator key + a real tenant.
- **AC4 (regression)**: 131 unit tests green. Full `scripts/smoke.sh` passed
  on the durable host (48 checks) — auth boundary, guard fallback, opt-out,
  dashboard, operator surfaces all unchanged; `seed-tenant.sh` mints api_keys
  rows; both real keys (first-real-developer + the ITEM-018 staged key)
  still authenticate (dashboard 200) post-migration.
- Ops notes (recorded): the redeploy rebuilt on the pinned
  `rust:1.98-bookworm` builder — the trixie/glibc transitional state from
  ITEM-027 is resolved (running image now matches the cc-debian12 runtime).
  The live run surfaced a pre-existing flaky smoke arm: `curl | grep -q`
  under `pipefail` fails whenever `grep -q` matches late in a still-streaming
  body and closes the pipe → curl dies SIGPIPE (exit 23). Fixed by capturing
  the body first (dashboard empty-state + shell arms).
