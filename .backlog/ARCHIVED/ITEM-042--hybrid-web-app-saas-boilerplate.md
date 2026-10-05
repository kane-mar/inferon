## ITEM-042: Hybrid web app — adopt SaaS-Boilerplate for the human surfaces

**As a** product owner
**I want** a proper Next.js web app for the human-facing surfaces
**So that** the product presents like a real SaaS instead of server-rendered
Rust HTML tables — without putting the data plane at risk

**Status:** complete (042a–f) — archived 2026-10-05. All six slices landed green:
`web/` vendored + stripped (a–c), real landing + session-gated dashboard +
operator console (d), live deploy behind Caddy (e), CI + README (f).
Rust `/auth/me` reports `is_operator` (TDD).

**Decision (A1, default): keep our GitHub OAuth + sessions** (`/auth/*` on the
Rust proxy). Clerk is stripped in 042c — the accounts horizon ITEM-034..040 is
live and would otherwise be stranded. Reversible later: auth sits behind the
API-client seam.

### Architecture

- Same repo; the app lives in `web/`. Two containers behind the existing Caddy:
  - `web` (Next.js, standalone build) — pages/SSR;
  - `proxy` (Rust) — `/v1/*` (OpenAI-compatible API + dashboard JSON),
    `/auth/*` (OAuth + session cookies), `/healthz`.
- Caddy routes `/v1/*`, `/auth/*`, `/healthz` → `proxy`; everything else → `web`.
- The app never verifies sessions itself: server-side it calls the Rust API on
  the compose network with the browser's `Cookie` forwarded (the API client
  seam); client-side it calls the same public origin.
- Nothing in the data plane changes; migrations/telemetry/CI stay.

### Slices (TDD; each lands green)

- **042a scaffold (this slice)**: vendor the template under `web/` (keep the MIT
  `LICENSE`/attribution), establish the local baseline (`npm ci`, `npm test`,
  `npm run build`) and record what gets stripped. No deploy changes yet.
- **042b API-client seam (TDD)**: `web/src/lib/inferon-api.ts` — typed client for
  `/auth/me`, the tenant dashboard JSON, keys/members, and the operator views,
  forwarding cookies; Vitest unit tests (mapping + error paths) against a mocked
  `fetch`. This is the boundary the UI talks to; Clerk never appears here.
- **042c strip vendor pieces** we don't use: Clerk (→ our session/redirects),
  Stripe, Drizzle + `migrations/` (our SQL schema is authoritative), Sentry,
  Crowdin, Checkly, the template's `.github/workflows` (ours wins) — plus the
  pages that depend on them.
- **042d pages (TDD)**: landing + sign-in (→ `/auth/login`), dashboard (summary,
  by-model, by-prompt, waste, keys, members), operator console (platform views)
  with role-aware nav. Needs one small Rust change: expose `is_operator` on
  `/auth/me` (TDD in `src/auth.rs`).
- **042e deploy**: `web` Dockerfile (standalone), compose service under the
  `prod` profile, Caddy path routing, health check; `scripts/smoke.sh` gains a
  web-surface arm.
- **042f CI**: the pipeline builds/tests the web app (`npm ci && npm test`),
  keeps `cargo test`, and the deploy health check covers both containers; README
  updated (architecture + local dev).

### Technical notes

- Template: Next 16, React 19, Node ≥ 24 (CI + image need Node 24; local dev
  too), Tailwind + shadcn/ui, next-intl, Vitest + Playwright, Storybook.
- MIT licence: keep the upstream `LICENSE` and attribution when vendoring.
- Supersedes nothing in the recorded product decisions; the "Rust + bash only"
  decision gains a scoped exception: **TypeScript allowed for the web UI**
  (`web/` only). Data plane stays Rust.

## Verification record (2026-10-05)

- **042a scaffold**: SaaS-Boilerplate vendored under `web/` with MIT `LICENSE`
  + attribution kept; baseline `npm ci` / `npm test` / `npm run build` recorded.
- **042b API-client seam (TDD)**: `src/libs/InferonApi.ts` (typed `/auth/me`,
  dashboard, keys/members, operator views; cookie forwarding) + Vitest unit
  tests against a mocked `fetch`; `session.ts` + `dashboardNav.ts` added in 042d.
- **042c vendor strip**: Clerk/Stripe/Drizzle+Sentry/Crowdin/Checkly + dependent
  pages removed; sign-in/up redirect to `/auth/login`; `Env.ts` trimmed.
- **042d pages (TDD)**: landing rebranded, session-gated dashboard (overview,
  keys, members) + operator console with role-aware nav; Rust `is_operator` on
  `/auth/me` (red→green). `cargo test --locked` = 190; web vitest 27; tsc/i18n/
  knip/eslint/next build green.
- **042e deploy**: standalone `web` image, compose `prod` service, Caddy routing
  (`/v1/*`/`/auth/*`/`/healthz` → proxy, else → web), stack + public health in CI,
  smoke web-surface arm. Live-verified (CI run 37254081389): `/` 200 + brand,
  `/dashboard` 307→`/auth/login`, `/auth/me` 401, `/auth/login` 302→GitHub,
  `/auth/callback` bad state 400, `/healthz` 200, `/v1/models` 404 (still Rust).
- **042f CI + README**: web job (`npm ci`, `next typegen`, tsc, `vitest --project
  unit`) alongside `cargo test --locked`; deploy health probes both containers;
  README documents the hybrid architecture + local dev + CI; `web/README.md`
  replaced with a stripped note. CI runs 37254916583 + 37256956942 green.
- Data plane untouched: no `/v1/chat` semantics changed; auth stays entirely in
  the Rust proxy (the web app forwards the session cookie, never verifies it).
