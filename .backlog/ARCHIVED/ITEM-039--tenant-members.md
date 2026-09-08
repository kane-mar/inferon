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

## Verification record (2026-09-07)

- **AC1 (claim-based invites)**: owner invites a GitHub login (migration 010
  tenant_invites); when that user signs in the callback claims the invite and
  joins the tenant as `member` — no signup screen, no provisioning, no second
  key (unit test + live smoke with a second mock identity). Owner can remove a
  member (owner cannot be removed — store + endpoint guard).
- **AC2 (member scope + removal)**: invited member's session is tenant-scoped
  (dashboard 200 live); removing them ends tenant scope on their next request
  (live: next dashboard call 401, session still verifies via /auth/me).
- **AC3 (boundaries)**: membership + keys + revoke endpoints are all scoped to
  the signed-in user's own tenant; cross-tenant revoke → 404 (unit). Invited
  members never inherit the inviter's key list beyond their own tenant scope.
- **AC4 (smoke)**: full lifecycle passed live — owner invites smoke-other →
  smoke-other signs in (mock identity #2, login marker on the authorize hop)
  and joins the owner's tenant → owner removes → next dashboard call is
  tenant-less. 65 smoke checks passed; no residue (0 users, 2 real tenants).
  Migration 010 applied live; no new migration needed beyond invites. 184 tests
  green.
