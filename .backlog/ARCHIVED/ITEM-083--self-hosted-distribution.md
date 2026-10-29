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

## Close-out (2026-10-29)

- **Profile** (AC1): `docker compose --profile self-hosted up -d` starts proxy +
  web + postgres + redis and nothing else — no Caddy, no cloud dependency. The
  web container is `profiles: ["prod", "self-hosted"]` and its host bind is now
  `${WEB_BIND:-127.0.0.1}` (loopback stays the default for `prod`; a
  self-hosted install sets `WEB_BIND=0.0.0.0` for LAN access). Absent
  integrations log and continue: the alert worker now says "alert webhook not
  configured — findings are evaluated and stored, not delivered" (OAuth and
  provider encryption already did).
- **Guide** (AC2/AC3): `guides/inferon-self-hosted.md` — prerequisites,
  install, secrets worth setting, first tenant + key, attaching a model on your
  network (`kind: local`, $0), first request, backup/restore (`pg_dump`/
  `pg_restore`), upgrade (pull → migrate → rebuild → verify) with the ordering
  rationale and rollback (additive migrations make the old binary safe), and
  air-gapped install (`docker save`/`load`, no egress). The support boundary is
  explicit: same artifact, self-supported, no licence key, no phone-home, no
  metering; what is given up is the hosted control plane, shared provider keys
  and the cross-tenant operator view.
- **CI** (AC4): a `self-hosted` job runs
  `docker compose --profile self-hosted config --quiet` and
  `scripts/smoke-selfhosted.sh`, which brings the stack up under its own
  compose project (`inferon-selfhosted-smoke`, ports 18080/13000), waits for
  `/healthz` + the web's `/api/health`, re-applies `scripts/migrate.sh` twice
  (the documented upgrade step must be idempotent), seeds a tenant through
  `seed-tenant.sh`, and proxies a real request through the mock model to a 200.
  Two bugs fixed while wiring it: an explicit `--profile` overrides
  `COMPOSE_PROFILES` (the mock never started), and the scripts were checked out
  `0644` (this workspace reports every file executable, so the bit was never
  recorded) — CI now calls them through `bash` and the index carries `0755`.
- **README** (AC5): Quickstart describes both profiles and
  "Running your own instance" links the guide. No new marketing surface.
- **No licence enforcement, telemetry opt-in or update checker** were built, as
  specified.
- **Live**: CI run `154` (`ac5e782`) — the `self-hosted` job passed end to end
  (image build → healthy stack → migrations → seeded key → proxied request);
  run `156` (`9c3eac2`) repeated it green alongside the production deploy.
