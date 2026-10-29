## ITEM-083: Self-hosted distribution (compose profile, install/upgrade, air-gapped)

**As a** team that cannot send prompts off its network but will run Docker
**I want** an honest, documented self-hosted install of the same product
**So that** "on your network" is a first-class deployment instead of a lucky
`docker compose up`

**Context (2026-10-28):** this is mostly already true. The image is the same,
`docker-compose.yml` already brings up `proxy` + `postgres` + `redis`, and
`--profile prod` adds `web` + `caddy`. What is missing is the *product surface*
around it: a named profile, a first-run path that does not assume the operator
knows the schema, an upgrade/rollback story, and an air-gapped story. No new
capability is claimed here.

**Acceptance Criteria**
1. A `self-hosted` compose profile (or documented equivalent) starts proxy +
   web + postgres + redis with **no cloud dependency and no outbound call** —
   including provider health probes and alerts, which must degrade quietly when
   no webhook is set.
2. `guides/inferon-self-hosted.md` covers, in order: prerequisites, install, an
   operator-created tenant + key, pointing a provider at a model on the same
   network (`kind: local`, $0 — ITEM-082), first proxied request, backup/restore
   of `pgdata`, upgrade (pull image → migrate → verify → rollback), and
   air-gapped install (pre-pulled images, no egress).
3. The guide states the support boundary plainly: same artifact, self-supported,
   no licence key, no phone-home, no metering — and what is therefore given up
   (hosted control plane, shared provider keys, the operator console's
   cross-tenant view).
4. CI validates the profile: `docker compose config` for the profile, plus a
   smoke that starts it, hits `/healthz`, and proxies one request end to end —
   so the documented path cannot rot unnoticed.
5. README links the guide; the landing page's "self-hosted" claim (if any)
   points at the same page. No new marketing surface.

**Technical Notes**
- Mostly docs + one compose profile; the only code is making absent integrations
  (alert webhook, GitHub OAuth, provider provisioning key) truly optional with a
  clear startup log line instead of a failure.
- Migrations must run from the shipped image (they already ship in `db/init` and
  the binary) — document the ordering, do not add a migration runner.
- Do not build licence enforcement, telemetry opt-in, or an update checker.

**Source:** 2026-10-28 customer-hosted connectivity review — profile 1
("Inferon on your network").
