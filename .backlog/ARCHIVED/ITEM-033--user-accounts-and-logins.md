## ITEM-033: User Accounts & Logins (captured — unrefined)

**Captured:** 2026-09-06 (backlog review, PO) — "also we'll need user accounts and logins."
**Type:** feature
**Status:** captured

**What this is NOT (duplicate avoidance):**
- Not ITEM-004 (archived/done) — that's machine auth: tenant API keys hashed in Postgres, Bearer swap on the proxy. No human login exists anywhere today.
- Not ITEM-018 — that mints a tenant key out-of-band (seed-tenant.sh runbook) and hands it to a developer; no account, no login, no self-serve.

**Open PO questions (refinement needed before it can rank or be pulled):**
1. ~~Who logs in — external developers only, or also operator accounts?~~ **ANSWERED (PO 2026-09-06): operator views move to accounts — yes.** The 4 operator JSON APIs (`/api/operator/*`, today authed by a shared `OPERATOR_API_KEY` env secret) become account-authenticated; that also buys the first human-facing operator screen (there is no operator HTML UI today — the browser page is tenant-only). **Sub-question ANSWERED (PO 2026-09-06): the shared `OPERATOR_API_KEY` survives for scripting/machine access alongside accounts** — accounts are for humans; the key stays for deploy scripts/cron/curl.
2. ~~Account model~~ **ANSWERED (PO 2026-09-06): tenants with multiple users — owner + members.** Accounts manage the tenant's API keys (create/revoke/rotate — today key minting is the operator-run `seed-tenant.sh` path).
3. ~~Signup path~~ **ANSWERED (PO 2026-09-06): self-serve signup** — developer signs up → tenant + key auto-provisioned.
4. ~~Mechanism~~ **ANSWERED (PO 2026-09-06): OAuth, GitHub initially.**
5. ~~Ranking~~ **ANSWERED (PO 2026-09-06): “wherever it's the least responsible moment”** — reading as least-disruptive position in the sequence (large work, don't let it block the small pullable cluster). Placed low; breaks down when it approaches the top.

**Placement default:** bottom of pullable region (large, unrefined — progressive granularity). PO ranking guidance 2026-09-06: “wherever it's the least responsible moment” (read: least-disruptive position; keep small pullable work flowing). Remains at bottom pending confirmation of that reading.

> **SUPERSEDED 2026-09-06** — refined into pullable backlog items ITEM-034..040
> (api keys as rows, GitHub OAuth + sessions, self-serve signup, operator
> console, tenant key management, tenant members, migrate out-of-band
> onboarding). This file keeps the parent capture + the PO answers.
