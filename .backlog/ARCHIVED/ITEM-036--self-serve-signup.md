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

## Verification record (2026-09-07)

- **AC1 (auto-provision, unit + live)**: a first-time /auth/callback now
  provisions — tenant row (on the platform shared upstream key,
  `UPSTREAM_MASTER_KEY`), owner membership (tenant_members, migration 008),
  and a first key through the ITEM-034 mint path shown **exactly once** on an
  HTML success screen ("Stored hashed, not shown again", copy button); the
  session cookie is set at the same time. Tests assert the key appears once in
  the page, resolves via tenant_by_api_key_hash, and that exactly one key row
  exists (hash-only).
- **AC2 (session-scoped dashboard)**: dashboard `resolve_scope` gained a
  session path — bearer (operator key, tenant key) is tried first and wins;
  otherwise the session cookie → user → tenant membership grants tenant scope.
  Signed-in owners/members reach their dashboard with no API key; key auth is
  unchanged (SDK/scripts unaffected). Shell loads key-less when signed in
  (ITEM-036 html + route-seam tests).
- **AC3 (README)**: "For developers" now leads with self-serve sign-in →
  copy key → 2-minute SDK example; the seed-tenant runbook is marked
  operator/machine-only (full demotion lands in ITEM-040).
- **AC4 (smoke, mock OAuth)**: OAuth endpoints are env-overridable
  (GITHUB_AUTHORIZE_URL/TOKEN_URL/API_URL) and the binary gained a
  `--mock-github` mode (compose profile `mock`, one fixed identity). The smoke
  runs a real curl OAuth dance and asserts: success screen with the raw key
  once, tenant + owner membership + one hashed key in Postgres, /auth/me
  resolves the session, the revealed key gets a real HTTP 200 (guarded
  replay), and a second sign-in lands on the dashboard without minting another
  key. ALL SMOKE CHECKS PASSED live (54 checks); smoke cleanup leaves no user
  / membership / tenant residue. Migration 008 applied live; 162 tests green.
- Live status: real self-serve signup needs the PO-registered GitHub OAuth app
  + `UPSTREAM_MASTER_KEY` on the durable host (until then /auth/login 503 is
  the live, correct behavior). Prod host migration 008 applied; unconfigured
  surface re-verified (503); real tenant keys still 200 after the redeploy.
