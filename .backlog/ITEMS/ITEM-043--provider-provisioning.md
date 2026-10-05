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
