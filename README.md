# Inferon

Drop-in OpenAI-compatible LLM proxy with per-tenant auth, async usage telemetry, cost
linting, and a self-hosted cost-leak dashboard.

**Status**: Slice 1 delivered (proxy + telemetry + cost visibility). Backlog in
`BACKLOG.md`; item files in `.backlog/`.

## What it does

- `POST /v1/chat/completions` — OpenAI SDK–compatible passthrough. Streaming requests
  pipe upstream bytes through zero-copy (SSE, `text/event-stream`); non-streaming
  responses pass through as JSON. TTFT is measured at the first upstream chunk.
- **Tenant auth** — every request needs `Authorization: Bearer <tenant key>`. Keys are
  SHA-256 hashed and stored as `api_keys` rows; a tenant can hold several keys and each
  is revocable individually (a revoked key stops authenticating immediately). The
  upstream `Authorization` header is swapped to the tenant's master key (client keys
  never reach the upstream and are never logged).
- **Telemetry** — every completed request (stream or not, success or error) is traced
  asynchronously into Postgres `traces`: tokens (usage parsed from the final SSE chunk
  for streams), ttft, total duration, raw prompt/completion, per-trace cost
  (`rust_decimal`, never float), waste flags, and a `truncated` marker for aborted
  streams.
- **Cost linter** (background worker): flags over-provisioned gpt-4o calls, repeated
  4k+ token uncached system prompts, and gpt-4o-on-simple-text misuse, with estimated
  $ impact.
- **Dashboard** — `GET /v1/dashboard` (self-contained HTML + JSON APIs), tenant-scoped.

## Architecture

Two deployment units behind one Caddy TLS ingress (compose profile `prod`):

- **`proxy` (Rust)** — the data plane: `/v1/*` (OpenAI-compatible API + dashboard
  JSON), `/auth/*` (GitHub OAuth + signed sessions), `/healthz`. Untouched by the
  web app.
- **`web` (Next.js)** — the human surfaces: rebranded landing, the session-gated
  dashboard (`/dashboard`, `/dashboard/keys`, `/dashboard/members`), and the
  operator console (`/dashboard/operator`). Server-side it calls the Rust API over
  the compose network (`INFERON_API_URL`) forwarding the browser's session cookie;
  the browser calls the same public origin. The app never verifies sessions itself —
  auth stays entirely in the Rust proxy.

Caddy routes `/v1/*`, `/auth/*`, `/healthz` → `proxy`; everything else → `web`
(Next.js standalone image on a loopback-only port).

## For developers

**Base URL:** `https://inferon.agilefederation.com/v1` (OpenAI-compatible).

### 1. Sign in (self-serve)

Open `https://inferon.agilefederation.com/auth/login` and sign in with GitHub.
First-time sign-in **auto-provisions your tenant** and shows your API key exactly
once (stored hashed, never shown again) — copy it there. Returning sign-in drops
you straight on your dashboard.

**Supported surface:**

- `POST /v1/chat/completions` — chat completions, streaming and non-streaming
  (`stream: true|false`), OpenAI SDK-compatible
- `GET /v1/dashboard` — your cost dashboard (per-tenant; only your traffic; works
  with either your API key or your browser sign-in)
  - JSON: `/v1/dashboard/api/summary`, `/by-model`, `/by-prompt`, `/waste` (all with `?days=7|30|90`)

**2-minute example (OpenAI SDK):**

```python
from openai import OpenAI

client = OpenAI(
    base_url="https://inferon.agilefederation.com/v1",
    api_key="sk-inferon-...",   # the key shown once at sign-up
)

chat = client.chat.completions.create(
    model="deepseek-chat",
    messages=[{"role": "user", "content": "Say hi"}],
)
print(chat.choices[0].message.content)

# Streaming works the same way:
for chunk in client.chat.completions.create(
    model="deepseek-chat",
    messages=[{"role": "user", "content": "Say hi"}],
    stream=True,
):
    print(chunk.choices[0].delta.content or "", end="")
```

Point your existing OpenAI-SDK code at the base URL + key above and it works
unchanged — Inferon is a drop-in proxy.

## Quickstart (Docker)

```bash
cp .env.example .env        # PORT only; secrets live in the DB (see below)
docker compose up -d --build
curl http://localhost:8080/healthz   # → ok
```

The stack: `proxy` (axum), `postgres` (schema auto-applied from `db/init/` on first
boot), `redis` (reserved for later slices). The web app + Caddy run under the `prod`
profile (`docker compose --profile prod up -d --build`), matching the durable host;
without it you get the data plane only.

## Minting a tenant key (operator / machine-only runbook)

> Self-serve sign-in is the primary onboarding (see For developers above).
> `seed-tenant.sh` is the out-of-band path for operators provisioning tenants
> on behalf of others, and for machines/scripts — not what a developer should
> use day to day.

Tenant keys are stored **hashed**; the tenant's *upstream master key* (the key the
proxy uses to call the provider on the tenant's behalf) is stored plaintext in
`tenants.upstream_openai_key` (DeepSeek is OpenAI-compatible → this column). On the
self-serve path a new tenant starts on the platform's shared upstream key
(`UPSTREAM_MASTER_KEY`); a tenant with its own provider key replaces it here.

```bash
./scripts/seed-tenant.sh "<tenant master upstream key>"
# prints the developer-facing key, e.g. sk-inferon-<random>
```

The script creates a tenant and inserts `sha256(<dev key>)` as its first `api_keys`
row (label `default`); the upstream master key is stored on the tenant. Only the hash
is ever stored — the printed key is the only time the raw key exists. Developers
paste it as their API key and point their SDK at your proxy base URL.

To verify by hand:

```bash
KEY=$(./scripts/seed-tenant.sh sk-your-deepseek-master-key | tail -1)
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $KEY" \
  -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"Say hi"}]}'
```

### Rotating a tenant's master key

```sql
UPDATE tenants SET upstream_openai_key = '<new master key>' WHERE id = '<tenant uuid>';
```

Existing developer keys keep working (the hash is unchanged); only the upstream key
swaps.

### Pointing at a different upstream

Set `UPSTREAM_URL` (full chat-completions URL) when starting the proxy/compose, e.g.
`https://api.openai.com/v1/chat/completions`. Default:
`https://api.deepseek.com/v1/chat/completions`.

## Dashboard

`GET http://<host>:8080/v1/dashboard` in a browser; enter a tenant key when prompted
(kept in the page only, never stored). Views: spend by model, top system-prompt
clusters, and waste flags with estimated $ impact. Time window: 1/7/30 days.

### Signing in (GitHub OAuth, ITEM-035)

Humans can sign in with GitHub for browser sessions — sessions gate nothing yet
(the dashboard still reads your tenant API key; `/v1/chat` stays key-authed). With
`GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET` and `SESSION_SECRET` set:

- `GET /auth/login` — redirects to GitHub; unconfigured → HTTP 503
  (`oauth_not_configured`)
- `GET /auth/callback` — completes the dance, sets the signed session cookie
  (HttpOnly, SameSite=Lax, Secure over TLS), redirects to `/v1/dashboard`
- `GET /auth/me` — the signed-in user (`github_id`/`github_login`) or 401
- `POST /auth/logout` — expires the session cookie

Sessions are stateless signed cookies (HMAC-SHA256, 30-day expiry); a forged or
expired cookie is treated as signed out, never an error.

JSON APIs (all require `Authorization: Bearer <tenant key>` and are tenant-scoped —
no tenant can see another tenant's data):

- `GET /v1/dashboard/api/summary?days=7` (totals, tokens, avg cost/request, avg TTFT)
- `GET /v1/dashboard/api/timeseries?days=30&offset=0` — daily buckets for the
  charts; `offset` shifts the window back (`offset=days` fetches the prior
  period for the compare view)
- `GET /v1/dashboard/api/by-model?days=7`
- `GET /v1/dashboard/api/by-prompt?days=7&limit=10`
- `GET /v1/dashboard/api/waste?days=7`

## CI/CD (redeploy on every push)

`.github/workflows/ci-cd.yml` runs on every push to `master`: `cargo test --locked`
(Rust), a **web job** (`npm ci`, `next typegen`, `tsc`, Vitest) for the Next.js app,
then a **deploy job** that ships the runtime bundle (Rust + `web/`) to the durable
host and redeploys it — `docker compose up -d postgres redis` → `scripts/migrate.sh`
(idempotent re-apply of `db/init/*.sql`, so existing volumes pick up new
migrations; the Postgres entrypoint only auto-applies on a fresh volume) →
`docker compose --profile prod up -d --build` → `--force-recreate caddy` → a health
check that probes **both** containers (`:8080/healthz` + `:3000/api/health`) and a
public landing check.

Repository secrets to set (Settings → Secrets and variables → Actions):

| Secret | Purpose |
|---|---|
| `DEPLOY_HOST` | durable host IP (update after a rebuild/restore) |
| `DEPLOY_SSH_KEY` | private key for `root@$DEPLOY_HOST` (`~/.ssh/inferon_deploy`) |
| `INFERON_HOST` | public hostname (Caddy site address) |
| `OPERATOR_API_KEY` | operator credential (machine + console access) |
| `OAUTH_CLIENT_ID` / `OAUTH_CLIENT_SECRET` / `SESSION_SECRET` | human sign-in (optional; unset = 503 on `/auth/login`). Note: GitHub reserves the `GITHUB_` prefix for secret **names**, so the repo secrets are `OAUTH_*`; they are passed to the container as `GITHUB_CLIENT_ID`/`GITHUB_CLIENT_SECRET` (what the app reads). |
| `UPSTREAM_MASTER_KEY` | upstream key given to self-serve tenants (optional) |
| `OPERATOR_GITHUB_LOGINS` | comma-separated operator GitHub logins (optional) |
| `ALERT_WEBHOOK_URL` | cost-anomaly webhook (optional) |

The deploy job **skips with a notice** (rather than failing) until
`DEPLOY_HOST` + `DEPLOY_SSH_KEY` exist. Local deploys use the same path:
`./scripts/deploy-durable.sh up|redeploy` (provisioning attaches the
`VULTR_FIREWALL_GROUP` web-ingress firewall when set).

## Running your own instance

`docker compose up -d --build` on any Docker host (see Quickstart above). The proxy
listens on `PORT` (default 8080); point your base URL at `http://<host>:8080/v1`.

## Durable deployment (inferon.agilefederation.com)

`scripts/deploy-durable.sh` provisions and updates a permanent host (~$5-6/mo —
the first always-on spend; `teardown` stops billing). It runs the stack with
`restart: unless-stopped` + named volumes behind a Caddy TLS ingress
(compose profile `prod`) on a Vultr firewall allowing only 22/80/443.

```bash
INFERON_HOST=inferon.agilefederation.com ./scripts/deploy-durable.sh up  # provision + deploy
./scripts/deploy-durable.sh redeploy  # rebuild + update the stack (reuses the saved INFERON_HOST)
./scripts/deploy-durable.sh status    # containers + healthz probe
./scripts/deploy-durable.sh teardown  # destroy the host
```

The public hostname is `INFERON_HOST`, saved to `~/.cache/inferon-durable/hostname`
on first deploy; change it + `redeploy` to move the endpoint (the durable host
currently serves inferon.agilefederation.com). It resolves via a plain A record in
the parent zone's DNS (DNSimple for agilefederation.com) — no registrar delegation
involved. Caddy obtains a Let's Encrypt cert automatically once the A record points
at the host and ports 80/443 are open.
Operator work happens in the browser console first (sign in with an operator
GitHub login from `OPERATOR_GITHUB_LOGINS` → `https://inferon.agilefederation.com/v1/dashboard/operator`:
platform totals, per-tenant spend/requests/guard fallbacks, guard outcomes,
alerts). Machines/scripts keep the operator JSON APIs +
`OPERATOR_API_KEY` (`/v1/dashboard/api/operator/*`).

Backups: a Vultr snapshot captures the pgdata/redisdata volumes (crash-consistent;
Postgres replays WAL on restore). Take one before risky changes:

```bash
curl -fsS -X POST https://api.vultr.com/v2/snapshots \
  -H "Authorization: Bearer $VULTR_API_KEY" -H "Content-Type: application/json" \
  -d '{"instance_id":"<host id from deploy-durable.sh state>","description":"inferon-durable-$(date +%Y%m%d)"}'
```

Restore: deploy a new instance from the snapshot (Vultr console) — the data
volumes come back intact and the stack auto-starts (`restart: unless-stopped`).
The restored instance gets a **new IP**: update the `inferon` A record in the
parent zone's DNS (DNSimple for agilefederation.com) and
`~/.cache/inferon-durable/state` before pointing anything at it.

## Development

- Rust + bash only for the data plane (project decision); SQL for migrations.
  **Scoped exception: TypeScript is allowed in `web/` only** (the Next.js UI).
- Tests: `cargo test --locked` (all unit/integration seams run locally with a mock
  upstream — no Docker needed for the red→green loop).
- Web app (`web/`), Node ≥ 24: `npm ci` then `npm run dev` (dev server) or
  `npm run build && npm start`. Server data calls go through `INFERON_API_URL`
  (defaults to `http://proxy:8080` in compose; set it to a local proxy, e.g.
  `INFERON_API_URL=http://localhost:8080`). Checks: `npm run check:types` (tsc),
  `npx vitest run --project unit` (unit tests — what CI runs; no browser),
  `npm run check:i18n`, `npm run check:deps` (knip),
  `npm run lint`, `npm run build`. The repo sits on an SMB share where npm stalls —
  develop in a local copy
  (`rsync -a web/ ~/.cache/inferon-web-sandbox/ --exclude node_modules --exclude .next`).
- Deployment verification (schema, live end-to-end): `scripts/docker-host.sh verify`
  provisions a disposable Vultr host; `scripts/smoke.sh` exercises healthz, the auth
  boundary, the dashboard, and the web surface inside the real stack.

## Project layout

```
src/cost.rs      price table + cost math (rust_decimal, cache-hit aware)
src/linter.rs    waste rules (over-provisioned / cache-miss / model misuse)
src/dashboard.rs dashboard HTML + tenant-scoped JSON APIs
src/tenant.rs    tenant + API-key persistence (TenantStore: in-memory + Postgres)
src/telemetry.rs batched trace worker + Postgres sink
src/usage.rs     token-usage parsing (SSE + JSON)
src/lib.rs       composition root: AppState, router, proxy hot path, streaming
src/main.rs      wiring (env, Postgres pool, worker)
db/init/         idempotent migrations (001 schema … 010 tenant invites)
web/             Next.js human surfaces (landing, dashboard, operator console) —
                 vendored SaaS-Boilerplate, stripped + Inferon-branded (ITEM-042)
.backlog/        product backlog (single-ranked) + archived items + refinement log
wiki/ raw/       design knowledge base (Karpathy-style wiki + immutable sources)
```
