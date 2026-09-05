## ITEM-014: Durable Deployment of api.inferon.dev

**As a** product owner
**I want** the proxy reachable at `https://api.inferon.dev/v1` on a permanent host
**So that** developers can paste the baseURL and get the Slice 1 experience for real

**Acceptance Criteria:**
1. A permanent (non-ephemeral) host runs the compose stack; traffic served over HTTPS on api.inferon.dev
2. Postgres data survives restarts and host rebuilds (backups or managed volume story)
3. Deploy is repeatable from a script (extend `scripts/docker-host.sh` or a new `deploy.sh`) — no hand-typed SSH
4. Tenants can be minted via a documented path (see ITEM-011)
5. Cost of the running host is understood and acknowledged (business decision — this is the first always-on spend)

**Technical Notes:**
- All verification so far used disposable Vultr hosts (destroyed after each item); nothing is deployed durably
- Domain/TLS: needs DNS for api.inferon.dev + a certificate story (Caddy/Traefik or a cloud LB)
- Break this down further when it reaches the top of the backlog (DNS, host provisioning, TLS, backups, tenant minting are candidate slices)

## Breakdown (2026-09-04 — item reached the top)
- **014a DNS delegation** — inferon.dev is parked on sav.com coming-soon NS; needs the registrar to point the zone at Vultr DNS (ns1/ns2.vultr.com) or add an A record for api.inferon.dev at the registrar. User action at the registrar. Vultr-side zone + A record can be pre-created.
- **014b Durable host + stack** — permanent Vultr instance running compose with `restart: unless-stopped` + named volumes (data survives reboots); `scripts/deploy-durable.sh` makes it repeatable.
- **014c TLS ingress** — Caddy (compose profile `prod`) reverse-proxying api.inferon.dev → proxy:8080 with auto Let's Encrypt; activates once 014a lands.
- **014d Firewall** — Vultr firewall group allowing only 22/80/443 on the durable host (no public 8080).
- **014e Public verification** — https://api.inferon.dev/healthz + a real tenant request through the public URL; archive the item after this passes.
- **014f Backups** — documented: Vultr snapshots of the host (pgdata/redisdata volumes); ad-hoc until demand justifies automation.

## Verification log (2026-09-05)

- **014a Vultr side COMPLETED (was actually missing)**: yesterday's "staged" zone never landed — `GET /v2/domains` returned zero. Created `inferon.dev` zone + `A api -> 66.135.9.177` (also apex A) via raw API (MCP DNS adapter still buggy — 4 positional args). NS records (ns1/ns2.vultr.com) now ready to serve the moment the registrar delegates. Remaining: **registrar NS delegation at sav.com (user action)**.
- **014b/c/d verified live** on 272bd949 (66.135.9.177): 4/4 containers healthy; full-stack `docker compose restart` passed (AC2 restarts); firewall group 6d9e8b43 attached.
- **AC2 with real data**: 2 real traces + 1 system_prompt_counts row survived a full-stack restart; counts re-seeded at boot (ITEM-010 persistence confirmed on the durable host).
- **AC4 verified live**: tenant minted via `scripts/seed-tenant.sh` on the host (documented path), real DeepSeek master key round-trip verified (cmp match, never printed); real chat through the durable proxy returned 200 (deepseek-v4-flash, trace at $0.000005). Dev key saved to `~/.cache/inferon-durable/first-real-developer.key` (0600).
- **014f made real**: first snapshot `4dca04b7` requested (2026-09-05, pre-014e); exact snapshot + restore procedure documented in README (restore = new IP → update `api` A record).
- **014c Caddy**: prod LE account registered (acme-v02 dir present since first boot); ACME failures are DNS-only (challenges answered by sav.com parking, http01.sslpluslogic.com 404). Retries hourly; `redeploy` forces an immediate attempt once DNS delegates.
- **014e still pending** (gated on registrar delegation).
- **Backlog effect**: once the registrar flips NS and 014e passes, ITEM-014 archives and the backlog is empty → replenish (Slice 3 candidates).
