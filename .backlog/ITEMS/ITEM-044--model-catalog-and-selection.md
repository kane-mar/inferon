## ITEM-044: Model catalog & per-tenant selection

**As a** tenant owner
**I want** to pick which models my traffic may use
**So that** routing and budgets operate over a set I control, not an implicit default

**Acceptance Criteria:**
1. The dashboard lists the models available from the tenant's providers (a curated
   static catalog per provider kind; optional live discovery where the provider
   exposes `/models`).
2. The tenant enables/disables models and marks one as default per provider.
3. Each model shows the metadata a developer needs: context window, max output,
   input price / output price, and capabilities (reasoning, streaming, images).
4. Routing (ITEM-015/045) only selects **enabled** models for the tenant.
5. A request for a disabled model returns a clear, typed error (not a raw upstream 4xx).

**Technical Notes:**
- New `tenant_models` table (tenant_id, provider_id, model_id, enabled, is_default)
  + a static catalog module keyed by provider kind.
- Pricing metadata should share one source with `cost::price_for` (ITEM-006) so the
  dashboard and the cost engine can never disagree — the `deepseek-flash` overcharge
  (2026-10-05) is the cautionary tale.
- Depends on ITEM-043 (a model belongs to a provider).

**Source:** PO review 2026-10-06 — "select models".

## Progress (2026-10-06) — catalog + selection landed

- `src/models.rs`: static `CATALOG` (id/name/kind/context/max output/reasoning/
  images) with **prices sourced from `cost::price_for`** — one source of truth so
  the catalog and the cost engine can never disagree; `ModelStore` seam (Postgres
  + in-memory).
- migration 013 `tenant_models` (tenant, model, enabled, is_default; **absent =
  unrestricted**, so own `openai-compatible` ids need no rows); one default per
  provider kind.
- API: `GET /v1/dashboard/api/models` (the catalog for the tenant's provider
  kinds, merged with the flags) and `POST …/:model_id` (owner-only enable/disable
  + set default).
- **AC4** (routing only selects enabled): the router/budget override is dropped
  when it targets a disabled model; **AC5**: a request for a disabled model is
  refused with a typed `model_disabled` 400 (traced), not a raw upstream 4xx.
- UI: `/dashboard/models` + `ModelsPanel` (grouped by provider kind, price +
  context + capability chips, enable/disable, make-default) + a nav item; en/fr
  copy. Strict FE/BE split kept.
- Rust 196 tests, web 48, all checks + `next build` green.
