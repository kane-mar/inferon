## ITEM-042: Hybrid web app — adopt SaaS-Boilerplate for the human surfaces

**As a** product owner
**I want** a proper Next.js web app for the human-facing surfaces
**So that** the product presents like a real SaaS instead of server-rendered
Rust HTML tables — without putting the data plane at risk

**Status:** refined + in progress (042d). 042a–d landed: template vendored under
`web/` (MIT `LICENSE`/attribution kept), typed Inferon API seam
(`web/src/libs/InferonApi.ts` + Vitest), vendor strip (Clerk/Sentry/Drizzle/
Checkly/Crowdin gone), and the real pages: rebranded landing (sign-in →
`/auth/login`), session-gated dashboard (overview, keys, members) and the
operator console, with role-aware nav. Rust `/auth/me` now reports `is_operator`
(TDD). Remaining: 042e deploy, 042f CI/README.

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
