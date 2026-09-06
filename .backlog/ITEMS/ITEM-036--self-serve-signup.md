## ITEM-036: Self-serve signup — tenant + key auto-provisioned (Accounts · from ITEM-033)

**As a** developer who just found Inferon
**I want** to sign in with GitHub and get a working API key immediately
**So that** I can try the API without waiting on an operator to seed me

**Acceptance Criteria:**
1. A first-time sign-in (user with no tenant membership) auto-provisions: a new
   tenant, the user as its owner (via a `tenant_members` link table:
   tenant_id, user_id, role `owner`|`member`, created_at), and a first API key
   minted through the ITEM-034 key path — shown exactly once on a success
   screen (copy affordance + "stored hashed, not shown again"); the proxy
   accepts it immediately (200 on a real request)
2. Returning users land on their own dashboard; a signed-in owner/member can
   view their tenant scope through the session (dashboard `resolve_scope`
   gains a session path) while API-key auth keeps working unchanged for SDKs
   and scripts (additive — ITEM-007 AC5 isolation intact)
3. README "For developers" section: self-serve sign-in becomes the primary
   onboarding (sign in → copy key → 2-minute example); the out-of-band
   seed-tenant steps are marked operator/machine-only (fully demoted in
   ITEM-040)
4. Smoke (mock OAuth): a fresh sign-in → tenant + owner + key exist in Postgres
   → the revealed key authenticates 200 → the success screen never shows the
   key again; a second sign-in for the same user lands on the dashboard

**Technical Notes:**
- Parent: ITEM-033 (archived as superseded) — slice 3
- Depends on ITEM-034 (keys) + ITEM-035 (sessions/users)
- Ranking note (refinement 2026-09-06): signup ranks above the operator
  console because the product goal is demand-side — real external developers —
  and this is the mechanism that stops needing the PO's per-developer handoff
- Auto-provisioned tenants start unconfigured (no budget row, guard enabled by
  default) — matches today's seeded tenants
