## ITEM-043: Provider provisioning (bring your own provider)

**As a** tenant owner
**I want** to add my own LLM provider credentials in the dashboard
**So that** Inferon proxies through *my* provider account, not a shared platform key

Today every self-serve tenant runs on the shared `UPSTREAM_MASTER_KEY` pointing at
the platform's DeepSeek account. That is fine for a demo and untenable for a real
product: a tenant cannot bring their own account, cannot use their own quota, and
cannot choose a different vendor. This is the first thing a real user hits.

**Acceptance Criteria:**
1. A dashboard page (`/dashboard/providers`) lists the tenant's providers and adds
   one: `kind` (deepseek | openai | anthropic | openai-compatible), display name,
   base URL, API key (and optional headers/org).
2. Provider secrets are stored **encrypted at rest** and are never returned by any
   API — the UI shows only a masked hint (`sk-…ab12`) and a last-updated date.
3. The proxy resolves a tenant's provider (and its key) for a request; a tenant with
   no provider keeps today's behavior (shared platform key) — no regression.
4. A "Test connection" action performs a minimal call and reports ok/failure.
5. Removing or rotating a provider takes effect without a redeploy.

**Technical Notes:**
- New `providers` table (tenant_id, kind, name, base_url, key_ciphertext, nonce,
  metadata, created_at) + a key-encryption seam (env master key / libsodium or
  age). Never log key material.
- `AppState`/proxy picks the provider per request via a `ProviderStore` seam
  (mirrors `TenantStore`); the upstream `Authorization` swap already exists
  (ITEM-004) — this extends its source from the tenant row to a provider row.
- Supersedes the single `tenants.upstream_openai_key` column (migrate + backfill).
- First slice to pull: schema + store + proxy resolution + one provider kind
  (deepseek), before the dashboard page.

**Source:** PO review 2026-10-06 — "no ability to provision new providers".

## Progress (2026-10-06) — providers landed

- **043a schema + encryption + store**: `providers` table (migration 012);
  `src/secret.rs` (XChaCha20-Poly1305, `PROVIDER_ENCRYPTION_KEY`); `src/providers.rs`
  (`ProviderStore` seam + `PostgresProviderStore` + `InMemoryProviderStore`,
  tenant-scoped, masked `key_hint` only). TDD: 5 secret + 3 store tests.
- **043b proxy resolution**: `AppState.provider_store`; each request resolves the
  tenant's provider (`base_url` + decrypted key) and falls back to the tenant's
  shared `upstream_openai_key` + the global `UPSTREAM_URL` (no regression). The
  guard's frontier replay threads the same route. 3 route tests.
- **043c dashboard**: `/dashboard/providers` page + `ProvidersPanel` (owner
  add/remove, member view-only), API `GET/POST /v1/dashboard/api/providers` +
  `POST …/:id/remove`, a "Providers" nav item, en/fr copy.
- Rust 191 tests, web 48, all checks + `next build` green.
- **Remaining** at the time of 043c: AC4 "test connection".
- **043d AC4 test connection** (commit `e666ba9`): a per-provider Test action probes
  `GET <base>/models` with the stored key and reports ok/status or `unreachable`.
- **043e completion (2026-10-06, commit `eeda65d`)**:
  - **AC5 rotation**: `POST /v1/dashboard/api/providers/:id` (owner-only) updates
    name/base_url/key/metadata in place — no redeploy.
  - **AC2 last-updated**: `updated_at` column + shown in the list.
  - **AC1 metadata**: optional org + arbitrary upstream headers (JSONB), applied
    on every forwarded request and the test probe (migration 016).
- **AC1 scope note (PO 2026-10-06)**: the `kind` allowlist is
  `deepseek | openai | openai-compatible`. Native Anthropic is **not**
  OpenAI-shaped (`/v1/messages`, `x-api-key`, different envelope) and is a new
  adapter — captured separately as **ITEM-050**. This item is complete for the
  OpenAI-compatible kinds.
- Env: `PROVIDER_ENCRYPTION_KEY` wired through compose + CI + `deploy-durable.sh`;
  unset = provisioning disabled (fail-soft).
