# Inferon on your network (self-hosted)

This is the same image you would get from the hosted service, run on your own
Docker host: the proxy, the dashboard, Postgres and Redis. Nothing leaves your
network, and nothing calls home — there is no licence key, no metering and no
update check.

Read the last section before you install: self-hosted is **supported** but
**self-supported**, and it gives up the hosted control plane on purpose.

## What you get, and what you do not

| You get | You give up |
|---|---|
| The same Rust proxy, dashboard, cost audit and router | The hosted control plane (no shared provider keys, no cross-tenant operator view) |
| Your prompts stay on your network | Managed upgrades, backups and TLS — they are yours |
| Providers including a model on your own LAN (`kind: local`, $0) | GitHub sign-in (optional; an operator-minted key works without it) |
| Postgres telemetry you own | Support response times |

## Prerequisites

- Docker Engine 24+ with the Compose v2 plugin.
- ~2 GB RAM (the proxy is small; Postgres is the steady consumer).
- An OpenAI-compatible model endpoint you want to route to — on this host, on
  the LAN, or a cloud API. A local one needs no key (Ollama, LM Studio, vLLM).
- Optional: a hostname and TLS termination if more than your LAN will reach it.

## Install

```bash
git clone https://github.com/kane-mar/inferon.git
cd inferon
cp .env.example .env          # fill in the secrets you want (see below)
docker compose --profile self-hosted up -d --build
```

That starts `proxy` (`:8080`), `web` (`:3000`), `postgres` and `redis`. There is
no Caddy in this profile — `http://<host>:3000` is the dashboard and
`http://<host>:8080/v1` is the API. To reach the dashboard from the LAN:

```bash
WEB_BIND=0.0.0.0 docker compose --profile self-hosted up -d
```

> Put TLS in front of both ports before exposing them beyond a trusted network.
> The `prod` profile does this with Caddy — see `docker-compose.yml` — if you
> would rather copy a working setup.

### Secrets worth setting

`PROVIDER_ENCRYPTION_KEY` (32-byte hex) is required for tenants to store their
own provider credentials. `SESSION_SECRET` and `GITHUB_CLIENT_ID`/`SECRET` are
only needed for GitHub sign-in. `ALERT_WEBHOOK_URL` is optional: without it the
alert worker still evaluates and stores findings — it just does not deliver
them.

Everything not set is logged at startup as disabled, rather than failing the
boot.

## Create the first tenant

Self-serve sign-up needs GitHub OAuth. Without it, mint a tenant and key
out-of-band:

```bash
./scripts/seed-tenant.sh sk-your-upstream-master-key "my team"
# prints: developer API key: sk-inferon-…   (shown once, stored as a hash)
```

The master key is the upstream credential used when a tenant has no provider of
its own. A tenant that adds a provider uses theirs instead.

## Point it at your model

Dashboard → **Providers** → **Add provider** → kind `local`, base URL such as
`http://192.168.1.10:11434/v1`, key blank. The cloud (the proxy container) must
be able to reach that URL; only the endpoint you expose is touched.

The provider's models are then selectable everywhere a cloud model is, and their
traffic costs `$0.00` while the counterfactual (what the frontier would have
cost) is still attributed in the audit. The reachability badge is refreshed on a
cadence, so a model that goes dark is visible before a request fails.

To prefer it for simple traffic, set a **tier pair** on Routing
(`deepseek-v4-pro → qwen3:8b`, for example).

## First request

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer sk-inferon-…" \
  -H 'content-type: application/json' \
  -d '{"model":"qwen3:8b","messages":[{"role":"user","content":"hello"}]}'
```

## Backup and restore

Everything that matters is in the `pgdata` volume (tenants, provider
credentials, telemetry, cost history). Take a logical dump:

```bash
docker exec inferon-postgres-1 pg_dump -U inferon -d inferon -Fc > inferon-$(date +%F).dump
```

Restore into a fresh stack (after `docker compose --profile self-hosted up -d
postgres`):

```bash
docker exec -i inferon-postgres-1 pg_restore -U inferon -d inferon --clean --if-exists < inferon-2026-10-29.dump
```

Volume snapshots work too, but stop the stack first so the dump is consistent.

## Upgrade and roll back

Migrations are additive and idempotent (`ALTER TABLE … IF NOT EXISTS`), and they
run from the shipped files — there is no migration table and no separate runner.

```bash
git pull                                   # or unpack the new bundle
docker compose up -d postgres redis        # database up
./scripts/migrate.sh                       # apply db/init/*.sql in order
docker compose --profile self-hosted up -d --build
curl -fsS http://localhost:8080/healthz    # → ok
```

Order matters: migrate **before** the new binary serves traffic, because a new
binary may read a column the old schema does not have.

**Rollback**: check out the previous release and repeat the same steps. Because
migrations only add things, the older binary runs against the newer schema — the
one exception is a release that drops or renames a column, which the release
notes call out; in that case restore your pre-upgrade dump.

## Air-gapped install

Build and export on a connected machine with the same architecture:

```bash
docker compose --profile self-hosted build
docker save inferon-proxy inferon-web postgres:16-alpine redis:7-alpine \
  | gzip > inferon-images.tgz
```

On the isolated host:

```bash
gunzip -c inferon-images.tgz | docker load
docker compose --profile self-hosted up -d        # no --build, no network
```

The stack makes no outbound call at startup. Note two things that *do* need
network when you enable them: the optional prompt embedder downloads its model
artifact (`scripts/fetch-embedder.sh` — run it before exporting), and providers
you configure point at endpoints you choose.

## Support boundary

- **Same artifact.** Self-hosted is the same binary and dashboard as the hosted
  service; there is no separate "enterprise" build and no feature flag to buy.
- **No licence key, no phone-home, no metering.** Nothing is enforced or
  reported back.
- **Self-supported.** Upgrades, backups, TLS and capacity are yours. The
  repository's `scripts/` and this guide are the support surface.
- **The dashboard works offline** except for the model and provider endpoints
  you configure, and GitHub OAuth if you enable it.

## See also

- [Customer-hosted LLMs](../wiki/llm-inference/customer-hosted-llms.md) — the
  three connectivity profiles and their tradeoffs.
- `README.md` § Durable deployment — the reference `prod` profile with Caddy.
