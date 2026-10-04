## ITEM-041: Durable host lost — restore vs rebuild (production incident)

**Captured:** 2026-10-04 (incident discovered while starting the ITEM-028 host
measurement)
**Type:** incident / ops
**Status:** **OPEN — PO decision required (spend + restore choice); production
is down**

**As a** product owner
**I want** the live endpoint back (or deliberately stood down)
**So that** `https://inferon.agilefederation.com/v1` isn't silently dead

### Facts (measured 2026-10-04)

- **Vultr account: 0 instances.** The recorded instance
  `272bd949-5243-4ca0-bd07-2139dfca2777` no longer exists (API: `instance not
  found`).
- **Account is healthy and paid** (balance $0, pending $0.09; last payment
  2026-10-01, $6.30) → this is *not* a billing suspension. The instance was
  deleted (console/API/automation). No teardown was run by the agent.
- **Endpoint down:** `https://inferon.agilefederation.com/healthz` times out;
  SSH and ICMP to `66.135.9.177` fail; the DNS A record still points at that
  (now dead) IP.
- **Snapshots survive** (3 in the account):
  - `4dca04b7` **2026-09-05** — `inferon-durable-2026-09-05 (pre-014e; pgdata+redisdata volumes, 2 traces, 1 tenant)` ← the only inferon snapshot
  - `98a531e2` 2025-11-05 (no description)
  - `cf7f5269` 2026-05-30 — auto-backup for a different host (`newsletter.agilefederation.com`)
- **Data impact:** everything since 2026-09-05 is lost — the second
  real-developer tenant and later traces. (Users/memberships/keys/invites
  created by the smoke runs were cleaned up per-run, but the ITEM-018
  staged-key tenant and the second real tenant existed only on that host.)
  Staged keys remain as local files, but their tenant rows were created
  2026-09-06 → those keys would 401 until their tenants are restored/re-created.

### Options (PO decision)

- **R1 — restore from snapshot `4dca04b7`:** create an instance from the snapshot
  (pgdata/redisdata as of 2026-09-05 → 1 tenant, 2 traces), redeploy the current
  code, then apply migrations 006–010 by hand (initdb does not re-run on an
  existing volume), point DNS at the new IP, update
  `~/.cache/inferon-durable/state`. Recovers the 09-05 tenant only; the 09-06
  tenant/staged keys remain lost.
- **R2 — rebuild fresh (no snapshot):** new instance + deploy current code →
  `db/init/001..010` apply on a clean volume (0 tenants). Re-mint only what's
  needed (`seed-tenant.sh`) or rely on self-serve. Cleanest; ~$6/mo.
- **R3 — stay down:** if Inferon isn't being pursued right now.

Either restore path costs ~$6/mo (vc2-1c-1gb) and ~20–40 min (build + DNS +
verification). The accounts-horizon human surfaces were never flipped live (no
GitHub OAuth app / env), so the practical loss is small.

### Blocks

- ITEM-028 host latency measurement (and its final AC number).
- Any live verification / smoke run.

### Acceptance (once the PO picks)

1. Chosen path executed; `/healthz` 200 locally and over the public TLS URL.
2. Migrations 001–010 present on the volume; real tenant key(s) authenticated.
3. `~/.cache/inferon-durable/state` + DNS updated; a snapshot taken after the
   restore/rebuild.
