## ITEM-039: Tenant members (Accounts · from ITEM-033)

**As a** tenant owner
**I want** teammates to sign in as members of my tenant
**So that** the tenant isn't bound to one person's GitHub login or one shared key

**Acceptance Criteria:**
1. Owner adds a member by GitHub login (the invite mechanism decision from
   ITEM-033, resolved for a no-email world: claim-based — the owner names a
   GitHub login; when that user signs in they join the tenant as `member`);
   owner can remove a member
2. A member signing in sees the tenant dashboard in tenant scope (ITEM-036
   session scoping) and the ITEM-038 key view with member permissions; removing
   a member ends their tenant scope on their next request (their sessions stay
   valid for the platform only — no tenant data)
3. Tenant boundaries hold: a member of tenant A can never see tenant B's rows
   through session or key paths (existing isolation tests extended to sessions)
4. Smoke: owner adds member login → member signs in (mock OAuth) → sees tenant
   A → owner removes → member's next dashboard call is 403/tenant-less

**Technical Notes:**
- Parent: ITEM-033 (archived as superseded) — slice 6
- Depends on ITEM-034..036 (tenant_members + keys + sessions)
- Decision recorded: no email infra exists (Rust + bash only, no external mail
  provider) — membership is GitHub-claim-based, which also matches OAuth-only
  signup; an invite-code/email path can be added if a real need appears
- Owner = the member who provisioned the tenant at ITEM-036 (first owner);
  ownership transfer is out of scope unless a PO need appears
