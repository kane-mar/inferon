## ITEM-038: Tenant key management (dashboard) (Accounts · from ITEM-033)

**As a** tenant owner
**I want** to create, label, and revoke my tenant's API keys from the dashboard
**So that** key lifecycle stops being an operator/SQL action

**Acceptance Criteria:**
1. Dashboard "Keys" section for the signed-in tenant: list keys (label,
   created, last used, revoked state), create a key (label + shown once),
   revoke a key — revocation takes effect immediately (the next request with
   that key is 401, traced as before)
2. Permission split (decision recorded): the tenant owner manages keys; members
   can view + create keys but not revoke (owners-only on destructive actions);
   enforced in the API, not just the UI
3. The minted-key flow reuses the ITEM-034 operator mint path (same
   semantics/hashing) behind the session, so there is one mint implementation
4. Smoke: owner signs in → creates key → key works (200) → revoked → 401;
   member session can create but the revoke button/endpoint is 403

**Technical Notes:**
- Parent: ITEM-033 (archived as superseded) — slice 5
- Depends on ITEM-034 (keys API), ITEM-035/036 (sessions + owner link)
- Surfacing keys never shows hashes or full keys — only labels + lifecycle
- This replaces "operator-run seed-tenant.sh minting for humans" (the ITEM-033
  PO answer); seed-tenant.sh itself is demoted in ITEM-040

## Verification record (2026-09-07)

- **AC1 (dashboard keys)**: session-scoped tenant keys API + shell panel. A
  signed-in member/owner GET /v1/dashboard/api/keys (metadata only — no
  hashes/key material asserted), POST creates (raw key shown once), revoke
  takes effect immediately (next chat with the revoked key → 401, live smoke).
- **AC2 (permission split)**: enforced in the API — owner and members may
  create; revoke requires role owner (member → 403). Unit + live smoke (member
  create 200, member revoke 403). TenantStore::revoke_tenant_key scopes the
  revoke to the owner's tenant — a cross-tenant key id answers 404 (boundaries
  unit test).
- **AC3 (one mint path)**: operator and dashboard mints share mint_key_payload
  (same ITEM-034 hashing semantics); the dashboard reuses the tenant store's
  key rows.
- **AC4 (smoke)**: owner session (the mock signup developer) lists/creates (key
  gets a real guarded HTTP 200) → revokes (next request 401); member session
  (seeded user + openssl-minted cookie) creates but revoke is 403. Full smoke
  passed live (62 checks). No new migration. 176 tests green.
- Smoke design note: the earlier doubling of smoke-dev as operator conflicted
  with the owner role (operators are deliberately rejected from tenant
  management); the smoke now seeds a distinct operator user for the 037 arms.
