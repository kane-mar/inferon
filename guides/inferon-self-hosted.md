# Inferon on your own network (self-hosted)

This is the same image the hosted service runs, on your own Docker host: the proxy,
the dashboard, Postgres and Redis. Nothing leaves your network, and nothing calls
home — there is no licence key, no usage metering and no update check.

Read the last section before you install. Self-hosted is **supported** but
**self-supported**, and it gives up the hosted control plane on purpose.

## What you get, and what you give up

| You get | You give up |
|---|---|
| The same Rust proxy, dashboard, cost audit and router | The hosted control plane (no shared provider keys, no cross-workspace operator view) |
| Your prompts stay on your network | Managed upgrades, backups and TLS — they are yours |
| Providers on your own LAN too (`kind: local`, $0) | GitHub sign-in (optional; an operator-minted key works without it) |
| Postgres usage data that you own | Support response times |

## What you need

- Docker Engine 24+ with the Compose v2 plugin.
- About 2 GB of RAM. The proxy is small; Postgres is the steady user.
- An OpenAI-compatible model endpoint to route to — on this host, on the LAN, or a
  cloud API. A local one needs no key (Ollama, LM Studio, vLLM).
- Optional: a hostname and TLS if anything beyond your LAN will reach it.

## Install

```bash
git clone https://github.com/kane-mar/inferon.git
cd inferon
cp .env.example .env          # fill in the secrets you want (see below)
docker compose --profile self-hosted up -d --build
```

That starts `proxy` (`:8080`), `web` (`:3000`), `postgres` and `redis`. This profile
has no Caddy: the dashboard is `http://<host>:3000` and the API is
`http://<host>:8080/v1`. To open the dashboard to your LAN:

```bash
WEB_BIND=0.0.0.0 docker compose --profile self-hosted up -d
```

> Put TLS in front of both ports before you expose them outside a network you
> trust. The `prod` profile does it with Caddy — see `docker-compose.yml` — if you
> would rather copy a setup that already works.

### Secrets worth setting

`PROVIDER_ENCRYPTION_KEY` (32-byte hex) is required before a workspace can store
its own provider credentials. `SESSION_SECRET` and `GITHUB_CLIENT_ID`/`SECRET` are
only needed for GitHub sign-in. `ALERT_WEBHOOK_URL` is optional: without it the
alert worker still finds and stores problems, it just does not send them anywhere.

Anything left unset is logged at startup as disabled, rather than failing the boot.

## Create the first workspace

Self-serve sign-up needs GitHub OAuth. Without it, create a workspace and key from
the command line:

```bash
./scripts/seed-tenant.sh sk-your-upstream-master-key "my team"
# prints: developer API key: sk-inferon-…   (shown once, stored as a hash)
```

The master key is the provider credential used when a workspace has no provider of
its own. A workspace that adds a provider uses theirs instead.

## Point it at your model

Dashboard → **Providers** → **Add provider** → kind `local`, base URL such as
`http://192.168.1.10:11434/v1`, key blank. The proxy container must be able to
reach that URL; only the endpoint you expose is touched.

The provider's models can then be picked anywhere a cloud model can, and their
traffic costs `$0.00` while the audit still records what the frontier model would
have charged. The reachability badge refreshes on a schedule, so a model that goes
dark shows up before a request fails.

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

Everything that matters is in the `pgdata` volume: workspaces, provider
credentials, usage data and cost history. Take a dump:

```bash
docker exec inferon-postgres-1 pg_dump -U inferon -d inferon -Fc > inferon-$(date +%F).dump
```

Restore into a fresh stack (after
`docker compose --profile self-hosted up -d postgres`):

```bash
docker exec -i inferon-postgres-1 pg_restore -U inferon -d inferon --clean --if-exists < inferon-2026-10-29.dump
```

Volume snapshots work too, but stop the stack first so the copy is consistent.

## Upgrade and roll back

Migrations only add things, and they can be re-run (`ALTER TABLE … IF NOT EXISTS`).
They run from the files in the repo, so there is no migrations table and no separate
runner.

```bash
git pull                                   # or unpack the new bundle
docker compose up -d postgres redis        # database up
./scripts/migrate.sh                       # apply db/init/*.sql in order
docker compose --profile self-hosted up -d --build
curl -fsS http://localhost:8080/healthz    # → ok
```

The order matters: migrate **before** the new binary serves traffic, because a new
binary may read a column the old schema does not have.

**Roll back** by checking out the previous release and repeating the same steps.
Because migrations only add things, the older binary runs on the newer schema. The
one exception is a release that drops or renames a column; the release notes call
that out, and in that case restore your pre-upgrade dump.

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

The stack makes no outbound call at startup. Two things do need a network when you
turn them on. The optional prompt embedder downloads its model file
(`scripts/fetch-embedder.sh` — run it before exporting), and any provider you
configure points at an endpoint you choose.

## Support boundary

- **Same artifact.** Self-hosted is the same binary and dashboard as the hosted
  service. There is no separate "enterprise" build and no feature flag to buy.
- **No licence key, no phone-home, no metering.** Nothing is enforced and nothing is
  reported back.
- **Self-supported.** Upgrades, backups, TLS and capacity are yours. The
  `scripts/` directory and this guide are the support surface.
- **The dashboard works offline**, except for the model and provider endpoints you
  configure, and GitHub OAuth if you turn it on.

## See also

- `README.md` § Durable deployment — the reference `prod` profile with Caddy, for a
  host you keep online.
- [Your first request](using/first-request.md) — the five-minute path once the stack
  is up.

_Last verified 2026-10-29 against the documented compose stack (master fe7c760)._
