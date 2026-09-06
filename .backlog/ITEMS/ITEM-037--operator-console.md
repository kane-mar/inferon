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
