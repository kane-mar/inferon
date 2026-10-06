## ITEM-049: Own the app shell — retire the template, one operator console

**As the** product team
**I want** the app shell and operator surface to be Inferon-owned
**So that** nothing template-shaped or duplicated remains to confuse the code or the UX

The web app was vendored from `ixartz/SaaS-Boilerplate` (ITEM-042a) and stripped
of Clerk (042c), but the **shell is still template-derived**: the `(auth)` route
group, `dashboard/layout.tsx`, `DashboardHeader`, `MobileNavigation`, `TitleBar`,
`SlashIcon`, `CenteredContainer`, and template flourishes (the `/` separator,
shadowed header). The content is ours; the frame is not. Separately there are
**two operator consoles** — Rust `/v1/dashboard/operator` (bearer/operator-key,
system fonts) and Next `/dashboard/operator` (session, themed) — and the README
points at the Rust one.

**Acceptance Criteria:**
1. The app shell (auth route-group layout, dashboard layout/header/nav, title bar,
   nav affordances) is re-implemented as Inferon components with no
   template-shaped structure or decoration; the landing and dashboard share one
   shell vocabulary where it makes sense (header/footer/nav primitives).
2. **One operator console**: the Next `/dashboard/operator` is the single
   browser surface; the Rust HTML console is retired (or redirects there for
   sessions). The machine-facing operator **JSON APIs are unchanged**
   (`OPERATOR_API_KEY` keeps working).
3. No orphaned template components remain; `knip` stays clean; README + docs
   point at the single operator console.
4. The Rust console's rendering tests are retired or moved with the surface; the
   Rust test suite stays green.

**Technical Notes:**
- Mostly frontend: replace the shell components with Inferon-owned ones, delete
  the template ones, update the `(auth)`/`(marketing)` layouts.
- The Rust change is removing `console_html` (and its tests) + the
  `/v1/dashboard/operator` HTML route (keep `operator_guard`/JSON routes). The
  the Rust HTML `/v1/dashboard` shell (ITEM-007 legacy) is a candidate alongside
  it — decide whether the key-based HTML dashboard stays or also retires.
- Depends on nothing; ranks as housekeeping after the control-plane items.
- Low risk: no data-plane behavior change.

**Source:** PO review 2026-10-06 — "finish decoupling from the template shell".

## Progress (2026-10-06) — the backend now renders no HTML

PO directive: **backend all Rust, frontend all TypeScript, clean separation.**
The server-rendered HTML was removed from the Rust backend entirely:

- **Retired `/v1/dashboard`** (legacy ITEM-007 HTML shell) and
  **`/v1/dashboard/operator`** (ITEM-037 HTML console): deleted `const HTML`,
  `console_html`, `html_escape`, `ConsoleTenant`, `console_tenant_rows`,
  `console_guard_outcomes`, `index`, `operator_console`, their routes and their
  tests (Rust 193 → 180 tests, all green). The operator JSON APIs are unchanged.
- **Retired the Rust signup success screen** (`success_screen` + `SIGNUP_CSS`):
  `/auth/callback` now redirects to the frontend (`/dashboard` for returning
  users, `/dashboard/keys` for new sign-ups, which mint their first key there).
  `provision_first_tenant` no longer mints a key — the frontend keys page does
  (shown once). Docs + `scripts/smoke.sh` updated to match.

So **AC2 and AC4 are done**; the operator console is the Next
`/dashboard/operator`, and the backend is JSON-only. What remains of this item is
**AC1/AC3**: retire the template-shaped *shell* (the Next `(auth)` route group,
`dashboard/layout.tsx`, `DashboardHeader`, `MobileNavigation`, `TitleBar`,
`SlashIcon`, `CenteredContainer`) so the frontend's frame is Inferon-owned too.

### Completion (2026-10-06) — the shell is Inferon-owned

- **New `features/shell/`**: `Logo`, `Section`, `NavLink`, `MobileMenu`,
  `PageHeader`, `DashboardShell` — the shared shell vocabulary used by both the
  landing and the dashboard.
- **Marketing rebuilt on it**: `features/landing/{LandingHeader,Hero,Features,CTA,
  LandingFooter}`; the `templates/` directory is gone.
- **Dashboard**: `DashboardShell` replaces `DashboardHeader` + `SlashIcon` +
  `MobileNavigation`; `PageHeader` replaces `TitleBar` across the 8 dashboard
  pages. The `/` separator flourish is gone.
- **Auth**: the dead `(center)` group + `CenteredContainer` are gone; sign-in and
  sign-up live directly under `(auth)` (they redirect to the Rust `/auth/login`).
- **Removed orphans**: `components/{ActiveLink,Background,MenuToggle*}`, the
  `features/landing/Centered*` + `FeatureCard` + `CTABanner` + `Section`,
  `components/ui/separator.tsx` (+ the `@radix-ui/react-separator` dependency),
  and `hooks/UseMenu*`.
- **README** updated: `src/dashboard.rs` is JSON-only, migrations go to 016, the
  web shell is Inferon-owned.
- Checks: tsc / eslint / i18n / knip / vitest-unit and `next build --webpack`
  green; one new browser test (`features/shell/Logo.test.tsx`).

All four ACs are now met: one Inferon-owned shell, one operator console (Next
`/dashboard/operator`), no template components, Rust suite green.
