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

## Decisions recorded at pull (2026-09-07)

- **Cookie plumbing: hand-rolled** (`session.rs`). One read header + one write
  header for two cookies (session + one-time OAuth state) — smaller surface
  than a cookie crate. Only new dep: `hmac = "0.12"` (sha2 already present).
  Session token = `<user_id>.<expiry_hex>.<hmac_hex>` (HMAC-SHA256 over the
  payload with SESSION_SECRET); verification never errors — forged/expired →
  signed out.
- **Mock GitHub: in-test trait fake** (`GithubClient` seam, FakeGithub in
  tests). No server/ports; the real OAuth dance needs a registered GitHub app
  (PO dependency — live verification covers the unconfigured surface until
  creds exist).
- **Current-user resolution: request extractor** (`CurrentUser` via
  FromRequestParts), not middleware — no authZ exists yet; a middleware layer
  lands with ITEM-036/037 when routes actually gate.
- **Module layout**: `session.rs` (pure token/cookie core) + `auth.rs` (users
  store, OAuth client, `/auth/*` handlers). AppState gains one `accounts:
  Option<Arc<Accounts>>` field (None = unconfigured).
- Kept as a single backlog item; slices are the in-item TDD progression
  (session tokens → users store + migration 007 → OAuth client seam →
  `/auth/*` routes → dashboard shell state).

## Verification record (2026-09-07)

- **AC1 (OAuth dance, seams)**: /auth/login redirects to GitHub's authorize
  URL with the client id, an encoded redirect URI, a per-attempt CSRF `state`
  nonce, and scope read:user; the nonce also lands in a short-lived
  HttpOnly/SameSite=Lax oauth_state cookie. /auth/callback verifies the state
  against the cookie (mismatch → 400, no session), exchanges the code through
  the GithubClient seam (real impl: token POST + /user GET; tests use a fake —
  no network, ITEM-035 AC3), and upserts the `users` row keyed by github_id
  (migration 007, unique index). Upsert-on-real-PG verified via the throwaway
  schema check (one row per github_id, login refreshed, idempotent re-apply).
- **AC2 (session cookie)**: token = HMAC-SHA256-signed `<user_id>.<expiry>` —
  HttpOnly, SameSite=Lax, Secure over TLS (X-Forwarded-Proto https, i.e.
  behind Caddy), 30-day Max-Age. Forged/expired/unknown-user cookies are
  signed out (401) — never an error (unit + route tests). /auth/logout
  expires the cookie.
- **AC3 (unconfigured + mock)**: with no GITHUB_CLIENT_ID/SECRET + SESSION_SECRET,
  /auth/login answers 503 `oauth_not_configured` (live on the durable host).
  All dance flows run against the in-test fake GithubClient (no external
  calls). Migration 007 applied live; smoke passed live (51 checks) incl. the
  new arms: /auth/me 401 without a session, /auth/login 503, shell still 200.
- **AC4 (me + shell, no authZ change)**: /auth/me returns the signed-in user
  (200) or 401; the dashboard shell renders sign-in/log-out state client-side
  from a bearer-free /auth/me fetch (the tenant key never rides that call) —
  shell content test + live smoke. /v1/chat + dashboard/operator bearer gates
  are untouched (real tenant keys still 200 live after the redeploy).
- Live status: **real GitHub sign-in needs a registered OAuth app** (PO):
  set GITHUB_CLIENT_ID/GITHUB_CLIENT_SECRET/SESSION_SECRET on the durable host
  and re-deploy; until then the unconfigured surface (503/401) is the live
  behavior. 151 tests green.
