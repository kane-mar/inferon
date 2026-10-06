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
