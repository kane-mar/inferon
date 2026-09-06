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
