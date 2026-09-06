## ITEM-035: GitHub OAuth + sessions (Accounts · from ITEM-033)

**As a** developer or operator using Inferon
**I want** to sign in with GitHub and hold a browser session
**So that** there is a human identity behind dashboard and console actions

**Acceptance Criteria:**
1. `/auth/login` redirects to GitHub OAuth (env `GITHUB_CLIENT_ID` +
   `GITHUB_CLIENT_SECRET`, redirect URL derived from the public base);
   `/auth/callback` exchanges the code and upserts a `users` row keyed by
   GitHub id (github_id unique, github_login, created_at, last_login_at)
2. Sign-in sets a session cookie (HttpOnly, SameSite=Lax, Secure — Caddy TLS
   terminates); session = signed/expiring token (env `SESSION_SECRET`) carrying
   the user id; middleware resolves the current user; `/auth/logout` clears it;
   a forged/expired cookie is treated as signed-out, never an error
3. Unconfigured GitHub env → `/auth/login` answers a clear 503
   ("oauth not configured"); all flows run against a local mock GitHub OAuth in
   tests (no external calls — the mock_upstream precedent)
4. `/auth/me` returns the signed-in user (or 401); the dashboard shell shows
   sign-in/log-out state. NO authorization yet: `/v1/chat` stays key-authed
   (ITEM-004 semantics untouched); dashboard/operator scope changes land in
   ITEM-036/037

**Technical Notes:**
- Parent: ITEM-033 (archived as superseded) — slice 2
- Depends on nothing (users/sessions are independent of api_keys)
- Design principle (recorded): logins gate dashboards/consoles only — the
  proxy hot path stays machine-key-authed; humans get sessions, machines get
  keys (per the ITEM-033 PO answers)
- Sessions don't touch the proxy or telemetry hot paths — auth middleware on
  dashboard routes only
- Cookie plumbing: hand-rolled Set-Cookie parsing vs a cookie crate — pick the
  smaller surface (decision recorded when pulled)
