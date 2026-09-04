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
  SHA-256 hashed and looked up against `tenants.api_key_hash`; the upstream
  `Authorization` header is swapped to the tenant's master key (client keys never
  reach the upstream and are never logged).
- **Telemetry** — every completed request (stream or not, success or error) is traced
  asynchronously into Postgres `traces`: tokens (usage parsed from the final SSE chunk
  for streams), ttft, total duration, raw prompt/completion, per-trace cost
  (`rust_decimal`, never float), waste flags, and a `truncated` marker for aborted
  streams.
- **Cost linter** (background worker): flags over-provisioned gpt-4o calls, repeated
  4k+ token uncached system prompts, and gpt-4o-on-simple-text misuse, with estimated
  $ impact.
- **Dashboard** — `GET /v1/dashboard` (self-contained HTML + JSON APIs), tenant-scoped.

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

The script inserts `sha256(<dev key>)` into `tenants.api_key_hash` and stores the
master key. Developers paste the printed key as their API key and point their SDK at
your proxy base URL.

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
db/init/         idempotent migrations (001 schema, 002 extensions)
.backlog/        product backlog (single-ranked) + archived items + refinement log
wiki/ raw/       design knowledge base (Karpathy-style wiki + immutable sources)
```
