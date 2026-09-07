## ITEM-037: Operator console — platform role + first HTML screen (Accounts · from ITEM-033)

**As an** Inferon operator
**I want** a browser console behind my GitHub login
**So that** I can run the platform without curl + the shared secret in front of me

**Acceptance Criteria:**
1. Platform-operator role: a `users` row flagged operator (bootstrap: env
   `OPERATOR_GITHUB_LOGINS` — comma-separated GitHub logins read at boot, the
   same secret-less pattern as the operator key; an operator can also promote
   via the operator API). Decision recorded — the env-login bootstrap is the
   default; a promote endpoint can follow if the PO wants
2. The 4 `/api/operator/*` JSON APIs accept a signed-in operator session
   (resolve_scope session path); the shared `OPERATOR_API_KEY` keeps working
   for scripts/cron/curl (per the ITEM-033 PO answers — accounts are for
   humans, the key survives for machines)
3. First HTML operator screen: platform totals, per-tenant list (spend,
   requests, guard fallbacks), operator guard + alerts views — read-only at
   this slice, rendered server-side in the existing single-page style (no new
   asset/CDN deps)
4. Isolation regression: a normal tenant user's session on operator routes →
   403; an operator session can never reach another tenant's scoped rows except
   through the operator (platform-wide) views

**Technical Notes:**
- Parent: ITEM-033 (archived as superseded) — slice 4
- Depends on ITEM-035 (sessions) — reads ITEM-026's guard views
- This is the "operator views move to accounts" PO answer made real — the
  operator screen is the first human-facing staff UI
- Roles stay minimal: platform operator (this slice) vs tenant owner/member
  (ITEM-036/039) — no RBAC framework

## Verification record (2026-09-07)

- **AC1 (platform-operator role)**: `users.is_operator` (migration 009, applied
  live). Env bootstrap `OPERATOR_GITHUB_LOGINS` (comma-separated logins) is
  authoritative at every sign-in and backfilled to existing users at boot
  (`UserStore::promote_logins` — store tests: insert flag, re-sign-in
  authoritative, idempotent boot promote). No promote-via-API endpoint this
  slice (recorded as optional in the item; the column is ready for it).
- **AC2 (operator APIs accept sessions)**: `resolve_scope` session path now
  resolves the user row: an operator session → `Scope::Operator` (the shared
  `OPERATOR_API_KEY` path is untouched for machines). Live smoke: smoke-dev
  (the mock GitHub identity, listed in OPERATOR_GITHUB_LOGINS) opens the
  console + operator JSON APIs with its session cookie; the operator key still
  opens the console.
- **AC3 (server-rendered console)**: `GET /v1/dashboard/operator` — one
  self-contained page (totals, per-tenant spend/requests/errors/guard
  fallbacks, guard outcomes, alerts) rendered server-side from the same
  operator SQL (no JS data fetches, no new assets). Pure `console_html` render
  fn unit-tested with fixtures incl. an XSS escape case (hostile tenant id +
  alert payload are escaped, never executable).
- **AC4 (isolation)**: unit (operator session 403 on tenant routes; member
  session 403 on operator routes) + live smoke: a real tenant key → 403 on the
  console, and a **cryptographically valid tenant-member session cookie**
  (minted with the smoke's SESSION_SECRET via `openssl dgst -hmac`, verified by
  /auth/me → 200 smoke-member) → 403 on the console.
- Live: migration 009 applied; operator console 401 unauth + 200 with the real
  OPERATOR_API_KEY; real tenant keys 200 after the redeploy; smoke cleanup
  leaves no residue (0 users, 2 real tenants). ALL SMOKE CHECKS PASSED (58
  checks). 170 tests green.
