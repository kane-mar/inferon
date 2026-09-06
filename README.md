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

## For developers

**Base URL:** `https://inferon.agilefederation.com/v1` (OpenAI-compatible).

Your team's API key is issued out-of-band by an Inferon operator (runbook:
[`scripts/seed-tenant.sh`](scripts/seed-tenant.sh)) — no signup page yet. The key is
shown exactly once, at mint time; the proxy only ever stores its SHA-256 hash. If a
key is lost or leaked, an operator mints a replacement and revokes the compromised
key.

**Supported surface:**

- `POST /v1/chat/completions` — chat completions, streaming and non-streaming
  (`stream: true|false`), OpenAI SDK-compatible
- `GET /v1/dashboard` — your cost dashboard (per-tenant; only your traffic)
  - JSON: `/v1/dashboard/api/summary`, `/by-model`, `/by-prompt`, `/waste` (all with `?days=7|30|90`)

**2-minute example (OpenAI SDK):**

```python
from openai import OpenAI

client = OpenAI(
    base_url="https://inferon.agilefederation.com/v1",
    api_key="sk-inferon-...",   # the key your operator gave you
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
boot), `redis` (reserved for later slices).

## Minting a tenant key (the operator task)

Tenant keys are stored **hashed**; the tenant's *upstream master key* (the key the
proxy uses to call the provider on the tenant's behalf) is stored plaintext in
`tenants.upstream_openai_key` (DeepSeek is OpenAI-compatible → this column).

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

JSON APIs (all require `Authorization: Bearer <tenant key>` and are tenant-scoped —
no tenant can see another tenant's data):

- `GET /v1/dashboard/api/summary?days=7`
- `GET /v1/dashboard/api/by-model?days=7`
- `GET /v1/dashboard/api/by-prompt?days=7&limit=10`
- `GET /v1/dashboard/api/waste?days=7`

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
Operator dashboard: `https://inferon.agilefederation.com/v1/dashboard/api/operator/*`
with the saved `OPERATOR_API_KEY`.

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

- Rust + bash only (project decision); SQL for migrations.
- Tests: `cargo test` (all unit/integration seams run locally with a mock upstream —
  no Docker needed for the red→green loop).
- Deployment verification (schema, live end-to-end): `scripts/docker-host.sh verify`
  provisions a disposable Vultr host; `scripts/smoke.sh` exercises healthz, the auth
  boundary, and the dashboard inside the real stack.

## Project layout

```
src/cost.rs      price table + cost math (rust_decimal, cache-hit aware)
src/linter.rs    waste rules (over-provisioned / cache-miss / model misuse)
src/dashboard.rs dashboard HTML + tenant-scoped JSON APIs
src/lib.rs       proxy hot path, auth, telemetry worker, trace sink
src/main.rs      wiring (env, Postgres pool, worker)
db/init/         idempotent migrations (001 schema … 006 api_keys rows)
.backlog/        product backlog (single-ranked) + archived items + refinement log
wiki/ raw/       design knowledge base (Karpathy-style wiki + immutable sources)
```
