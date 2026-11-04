# Inferon

A proxy that sits between your app and an AI provider. It speaks the OpenAI API, so
your existing client keeps working. It records what every request really costs,
checks the answers that come back, and moves traffic to a cheaper model when your
budget runs low.

**Status**: Slice 1 is delivered (proxy + usage records + cost dashboard). The
backlog is in `BACKLOG.md`; item files are in `.backlog/`.

> A note on words: the product says **workspace**; the database, scripts and API
> paths still say **tenant** (`tenants` table, `scripts/seed-tenant.sh`). They are
> the same thing.

## What it does

- **`POST /v1/chat/completions`** — a pass-through any OpenAI SDK can call.
  Streaming responses (SSE, `text/event-stream`) are piped through byte for byte;
  non-streaming responses pass through as JSON. The time to the first token (TTFT)
  is measured at the first chunk from the provider.
- **Workspace auth** — every request needs `Authorization: Bearer <workspace key>`.
  Keys are stored as SHA-256 hashes in the `api_keys` table. A workspace can hold
  several keys and each can be revoked on its own; a revoked key stops working at
  once. The proxy swaps the header for the workspace's master key, so client keys
  never reach the provider and are never logged.
- **Usage records** — every finished request (streaming or not, success or error)
  is written to the Postgres `traces` table in the background. It stores the tokens
  (read from the last SSE chunk for streams), TTFT, total duration, the raw prompt
  and completion, the cost of that one request (`rust_decimal`, never a float), the
  waste flags, and a `truncated` marker for stopped streams.
- **Cost linter** (background worker) — finds three kinds of waste and estimates
  what each one costs: an over-provisioned gpt-4o call, the same 4k+ token system
  prompt sent again with no cache, and gpt-4o used on simple text.
- **Dashboard** — the TypeScript web app at `/dashboard` (one workspace at a time),
  backed by the JSON APIs under `/v1/dashboard/api/*`.

## Architecture

Two deployment units behind one Caddy TLS ingress (compose profile `prod`):

- **`proxy` (Rust)** — the data plane: `/v1/*` (the OpenAI-compatible API and the
  dashboard JSON), `/auth/*` (GitHub OAuth and signed sessions), `/healthz`. The
  web app never touches it directly.
- **`web` (Next.js)** — the human surfaces: landing page, the sign-in-only
  dashboard (`/dashboard`, `/dashboard/onboarding`, `/dashboard/providers`,
  `/dashboard/keys`, `/dashboard/members`) and the operator console
  (`/dashboard/operator`). When it renders a page it calls the Rust API over the
  compose network (`INFERON_API_URL`) and forwards the browser's session cookie;
  the browser talks to the same public origin. The web app never checks sessions
  itself — sign-in lives only in the Rust proxy.

**Providers (ITEM-043)**: a workspace can add its own provider at
`/dashboard/providers` (DeepSeek, OpenAI, or any OpenAI-compatible host). The keys
are encrypted at rest (XChaCha20-Poly1305 with `PROVIDER_ENCRYPTION_KEY`) and only a
masked hint can ever be read back. On each request the proxy uses the workspace's
own provider, and falls back to the shared platform key when the workspace has none.

Caddy sends `/v1/*`, `/auth/*` and `/healthz` to `proxy`, and everything else to
`web` (a Next.js standalone image on a loopback-only port).

## Performance contract

The proxy's own overhead is **p50 under 1 ms and p95 under 2 ms for a warm
request**. Overhead means the time from the start of a request to just before it is
sent to the provider, recorded as `traces.proxy_us`. It leaves out the provider and
the response body, and counts auth, workspace config, routing, the budget and router
decision, and parsing.

"Warm" has a precise meaning: the request lands inside the cache windows (keys and
spend 1 s, config 5 s), which means it is part of a burst. The **first** request
after a quiet period also loads the config, which takes about 14 ms on the 1 vCPU
host. A workspace that sends one request a minute therefore pays that cold cost, not
the warm overhead. The claim is about what the proxy adds to a request. It is not a
claim that Postgres is free.

This is measured on every change, not promised:

- the unit suite measures it on every push
  (`proxy_overhead_is_sub_millisecond_for_small_requests`);
- the deploy job runs `scripts/perf-check.sh` against the live host after every
  deployment, prints the measured numbers, and **fails the run if p50 ≥ 1 ms**.
  p95 is reported with a 5 ms guard rather than gated at 1–2 ms: on the shared
  1 vCPU host the tail depends on the scheduler (1.1–3.5 ms seen), while a cache
  regression measured 6–28 ms;
- the hot path makes **one** storage round trip per request (the key lookup).
  Everything else — controls, policy, fallbacks, route, warm target, spend — is
  served from `src/hotpath.rs` and refreshed on every write.

## For developers

**Base URL:** `https://inferon.agilefederation.com/v1` (OpenAI-compatible).

### 1. Sign in (self-serve)

Open `https://inferon.agilefederation.com/auth/login` and sign in with GitHub.
Your first sign-in **creates your workspace** and shows your API key exactly once
(it is stored hashed and never shown again) — copy it there. After that, signing in
takes you straight to your dashboard.

**What you can call:**

- `POST /v1/chat/completions` — chat completions, streaming and non-streaming
  (`stream: true|false`), OpenAI SDK-compatible
- `GET /dashboard` — your cost dashboard (one workspace, sign in with GitHub; the
  web app reads the JSON APIs below with your session cookie)
  - JSON: `/v1/dashboard/api/summary`, `/timeseries`, `/by-model`, `/by-prompt`,
    `/waste` (all with `?days=7|30|90`)

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

Point your existing OpenAI SDK code at the base URL and key above. It works
unchanged — Inferon is a drop-in proxy.

## Quickstart (Docker)

```bash
cp .env.example .env        # PORT only; secrets live in the DB (see below)
docker compose up -d --build
curl http://localhost:8080/healthz   # → ok
```

The stack is `proxy` (axum), `postgres` (the schema is applied from `db/init/` on
first boot) and `redis` (reserved for later slices). Two profiles add more:

- `--profile self-hosted` — proxy + dashboard (Next) + Postgres + Redis, with no TLS
  ingress. This is the "run it on your own network" install:
  **[guides/inferon-self-hosted.md](guides/inferon-self-hosted.md)**.
- `--profile prod` — the same stack behind Caddy, matching the durable host.

## Creating a workspace key (operator / machine-only runbook)

> Self-serve sign-in is the normal way in (see "For developers" above).
> `seed-tenant.sh` is the out-of-band path: for operators who create workspaces for
> other people, and for machines and scripts. A developer should not need it day to
> day.

Workspace keys are stored **hashed**. A workspace's *upstream master key* — the key
the proxy uses to call the provider for that workspace — is stored as plain text in
`tenants.upstream_openai_key`. That column is for the **platform's own** workspace
and for workspaces an operator seeds with `scripts/seed-tenant.sh`. Self-serve
workspaces are **keyless** and must add their own provider in the dashboard; the
platform key is not a pass-through.

```bash
./scripts/seed-tenant.sh "<tenant master upstream key>"
# prints the developer-facing key, e.g. sk-inferon-<random>
```

The script creates a workspace and inserts `sha256(<dev key>)` as its first
`api_keys` row (label `default`); the master key is stored on the workspace. Only
the hash is stored, so the printed key is the only time the raw key exists.
Developers paste it as their API key and point their SDK at your proxy base URL.

To check it by hand:

```bash
KEY=$(./scripts/seed-tenant.sh sk-your-deepseek-master-key | tail -1)
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $KEY" \
  -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"Say hi"}]}'
```

### Replacing a workspace's master key

```sql
UPDATE tenants SET upstream_openai_key = '<new master key>' WHERE id = '<tenant uuid>';
```

Developer keys keep working (the hash does not change); only the upstream key is
swapped.

### Pointing at a different provider host

Set `UPSTREAM_URL` (the full chat-completions URL) when you start the proxy or
compose, for example `https://api.openai.com/v1/chat/completions`. Default:
`https://api.deepseek.com/v1/chat/completions`.

## Dashboard

`https://<INFERON_HOST>/dashboard` is the Next.js app (sign in with GitHub). You
get KPI cards with period-over-period changes, a time-series chart, spend by model,
your top system prompts, and waste flags, over 7/30/90-day windows with a compare
toggle. The JSON APIs below back it and stay available to scripts.

### Signing in (GitHub OAuth, ITEM-035)

People sign in with GitHub for browser sessions. The dashboard requires a session;
`/v1/chat` stays key-authed. With `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET` and
`SESSION_SECRET` set:

- `GET /auth/login` — sends you to GitHub; if OAuth is not configured it answers
  HTTP 503 (`oauth_not_configured`)
- `GET /auth/callback` — finishes the sign-in, sets the signed session cookie
  (HttpOnly, SameSite=Lax, Secure over TLS) and redirects to `/dashboard` (new
  accounts land on `/dashboard/onboarding`, the guided first run)
- `GET /auth/me` — the signed-in user (`github_id`/`github_login`), or 401
- `POST /auth/logout` — expires the session cookie

Sessions are stateless signed cookies (HMAC-SHA256, 30 days). A forged or expired
cookie counts as signed out; it is never an error.

The JSON APIs all need `Authorization: Bearer <workspace key>` and show one
workspace only — no workspace can see another's data:

- `GET /v1/dashboard/api/summary?days=7` (totals, tokens, average cost per request,
  average TTFT)
- `GET /v1/dashboard/api/timeseries?days=30&offset=0` — daily buckets for the
  charts; `offset` moves the window back (`offset=days` fetches the previous period
  for the compare view)
- `GET /v1/dashboard/api/by-model?days=7`
- `GET /v1/dashboard/api/by-prompt?days=7&limit=10`
- `GET /v1/dashboard/api/waste?days=7`

## CI/CD (redeploy on every push)

`.github/workflows/ci-cd.yml` runs on every push to `master`. It runs
`cargo test --locked` (Rust), then a **web job** (`npm ci`, `next typegen`, `tsc`,
Vitest) for the Next.js app, then a **deploy job** that ships the runtime bundle
(Rust + `web/`) to the durable host and redeploys it. The job runs
`docker compose up -d postgres redis` → `scripts/migrate.sh` (which re-applies
`db/init/*.sql`, so an existing volume picks up new migrations; the Postgres
entrypoint only applies them on a fresh volume) →
`docker compose --profile prod up -d --build` → `--force-recreate caddy` → a health
check that probes **both** containers (`:8080/healthz` + `:3000/api/health`) and a
public landing check.

Repository secrets to set (Settings → Secrets and variables → Actions):

| Secret | Purpose |
|---|---|
| `DEPLOY_HOST` | durable host IP (update after a rebuild or restore) |
| `DEPLOY_SSH_KEY` | private key for `root@$DEPLOY_HOST` (`~/.ssh/inferon_deploy`) |
| `INFERON_HOST` | public hostname (the Caddy site address) |
| `OPERATOR_API_KEY` | operator credential (machine and console access) |
| `OAUTH_CLIENT_ID` / `OAUTH_CLIENT_SECRET` / `SESSION_SECRET` | human sign-in (optional; unset means `/auth/login` answers 503). GitHub reserves the `GITHUB_` prefix for secret **names**, so the repo secrets are `OAUTH_*`; they reach the container as `GITHUB_CLIENT_ID`/`GITHUB_CLIENT_SECRET`, which is what the app reads. |
| `UPSTREAM_MASTER_KEY` | platform key for operator-seeded workspaces only (`seed-tenant.sh`) — not a self-serve pass-through (optional) |
| `PROVIDER_ENCRYPTION_KEY` | 32-byte hex key that encrypts workspace provider credentials at rest (ITEM-043; unset disables provider provisioning) |
| `OPERATOR_GITHUB_LOGINS` | comma-separated operator GitHub logins (optional) |
| `ALERT_WEBHOOK_URL` | cost-anomaly webhook (optional) |

Until `DEPLOY_HOST` and `DEPLOY_SSH_KEY` exist, the deploy job **skips with a
notice** instead of failing. Local deploys take the same path:
`./scripts/deploy-durable.sh up|redeploy` (provisioning attaches the
`VULTR_FIREWALL_GROUP` web-ingress firewall when it is set).

## Running your own instance

`docker compose --profile self-hosted up -d --build` on any Docker host. Install,
upgrade, backup and air-gapped steps are in
[guides/inferon-self-hosted.md](guides/inferon-self-hosted.md). The proxy listens on
`PORT` (default 8080); point your base URL at `http://<host>:8080/v1`.

## Durable deployment (inferon.agilefederation.com)

`scripts/deploy-durable.sh` creates and updates a permanent host (about $5–6/month,
the first always-on spend; `teardown` stops the billing). It runs the stack with
`restart: unless-stopped` and named volumes behind a Caddy TLS ingress (compose
profile `prod`) on a Vultr firewall that allows only ports 22/80/443.

```bash
INFERON_HOST=inferon.agilefederation.com ./scripts/deploy-durable.sh up  # create + deploy
./scripts/deploy-durable.sh redeploy  # rebuild and update (reuses the saved INFERON_HOST)
./scripts/deploy-durable.sh status    # containers + healthz probe
./scripts/deploy-durable.sh teardown  # destroy the host
```

The public hostname is `INFERON_HOST`, saved to `~/.cache/inferon-durable/hostname`
on the first deploy. To move the endpoint, change it and run `redeploy` (the host
currently serves inferon.agilefederation.com). It resolves through a plain A record
in the parent zone (DNSimple for agilefederation.com), so no registrar delegation is
involved. Caddy gets a Let's Encrypt certificate by itself once the A record points
at the host and ports 80/443 are open.

Operators work in the browser console first: sign in with a GitHub login listed in
`OPERATOR_GITHUB_LOGINS` and open
`https://inferon.agilefederation.com/dashboard/operator` for platform totals,
per-workspace spend and requests, guard fallbacks, guard outcomes and alerts.
Machines and scripts keep the operator JSON APIs with `OPERATOR_API_KEY`
(`/v1/dashboard/api/operator/*`).

Backups: a Vultr snapshot captures the pgdata and redisdata volumes (crash
consistent; Postgres replays the WAL on restore). Take one before risky changes:

```bash
curl -fsS -X POST https://api.vultr.com/v2/snapshots \
  -H "Authorization: Bearer $VULTR_API_KEY" -H "Content-Type: application/json" \
  -d '{"instance_id":"<host id from deploy-durable.sh state>","description":"inferon-durable-$(date +%Y%m%d)"}'
```

To restore, deploy a new instance from the snapshot in the Vultr console. The data
volumes come back intact and the stack starts by itself (`restart: unless-stopped`).
The restored instance has a **new IP**, so update the `inferon` A record in the parent
zone (DNSimple for agilefederation.com) and `~/.cache/inferon-durable/state` before
anything points at it.

## Development

- Rust + bash for the data plane only (a project decision); SQL for migrations.
  **Scoped exception: TypeScript is allowed in `web/` only** (the Next.js UI).
- Tests: `cargo test --locked`. Every unit and integration seam runs locally with a
  mock provider, so the red→green loop needs no Docker.
- Web app (`web/`), Node ≥ 24: `npm ci`, then `npm run dev` for the dev server, or
  `npm run build && npm start`. Server-side data calls go through `INFERON_API_URL`
  (default `http://proxy:8080` in compose; set it to a local proxy, e.g.
  `INFERON_API_URL=http://localhost:8080`). Checks: `npm run check:types` (tsc),
  `npx vitest run --project unit` (the unit tests CI runs; no browser),
  `npm run check:i18n`, `npm run check:deps` (knip), `npm run lint`,
  `npm run build`. The repo sits on an SMB share where npm stalls, so develop in a
  local copy
  (`rsync -a web/ ~/.cache/inferon-web-sandbox/ --exclude node_modules --exclude .next`).
- Deployment checks (schema, live end-to-end): `scripts/docker-host.sh verify`
  creates a disposable Vultr host; `scripts/smoke.sh` exercises healthz, the auth
  boundary, the dashboard and the web surface inside the real stack.

### Prompt embedder (optional, `embedder` feature)

The task-aware router can embed prompts with a quantized BGE-micro ONNX model
(384-d). It is an **opt-in build**: the default build and the local test loop never
load ONNX Runtime.

```bash
./scripts/fetch-embedder.sh    # model + tokenizer + ONNX Runtime 1.19.2
# it prints the env vars to export, then:
cargo test --features embedder --release -- --ignored --nocapture embed_bench
cargo test --features embedder --release -- --ignored --nocapture embed_semantics
```

- ONNX Runtime **1.19.2** is pinned (the `ort` feature `api-19`): the newest release
  that still ships macOS **x86_64** and targets macOS 11. Newer builds need macOS 13
  libc++ symbols.
- Runtime only: `ort` uses `load-dynamic`, so nothing links at build time and
  `ORT_DYLIB_PATH` points at the downloaded library.
- Measured on the dev Mac (128-token window, level-3/all optimizations, 4 intra
  threads): **p50 ~8–10 ms**, p95 ~12–14 ms per embed. It is a task-level signal,
  cached per system prompt, never computed per request.

## Project layout

```
src/cost.rs      price table + cost math (rust_decimal, cache-hit aware)
src/linter.rs    waste rules (over-provisioned / cache-miss / model misuse)
src/dashboard.rs workspace-scoped + operator JSON APIs (no HTML — the frontend owns the UI)
src/tenant.rs    workspace + API-key persistence (TenantStore: in-memory + Postgres)
src/telemetry.rs batched trace worker + Postgres sink
src/usage.rs     token-usage parsing (SSE + JSON)
src/lib.rs       composition root: AppState, router, proxy hot path, streaming
src/main.rs      wiring (env, Postgres pool, worker)
db/init/         idempotent migrations (001 schema … 016 provider metadata)
web/             Next.js human surfaces (landing, dashboard, one operator console);
                 Inferon-owned shell (ITEM-049)
.backlog/        product backlog (single ranked list) + archived items + refinement log
wiki/ raw/       design knowledge base (Karpathy-style wiki + immutable sources)
```
