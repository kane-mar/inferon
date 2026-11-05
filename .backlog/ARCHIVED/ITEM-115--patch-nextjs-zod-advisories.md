## ITEM-115: Patch Next.js and zod for the 2026-11 security advisories

**As the** operator of the live site
**I want** the web image to run patched versions of Next.js and zod
**So that** a public RCE advisory and an input-validation memory-exhaustion bug are
not sitting in the container that faces the internet

**Context (2026-11-05, found during the ITEM-111 licence audit).** `npm audit
--omit=dev` in `web/` reports two vulnerabilities in production dependencies:

| Package | Installed | Advisory range | Severity | Fixed in |
|---|---|---|---|---|
| `next` | 16.3.8 | 16.0.0 – 16.4.0 | critical | 16.4.5 |
| `zod` | 4.6.5 | < 4.6.6 | high | 4.6.7 |

The Next.js advisories are an unauthenticated RCE in the Image Optimization API
when SVG is used, a denial of service in App Router Server Actions, and an RCE in
`use cache` functions returning typed arrays. The zod advisory is memory exhaustion
while validating oversized constrained collections. The app does not use `use
cache` or SVG image optimisation today, but the versions are shipped and the fix is
a patch bump.

**Acceptance Criteria**
1. `web/package.json` and `web/package-lock.json` raise `next` to 16.4.5 or newer
   and `zod` to 4.6.7 or newer, with no other dependency moved unless required by
   those bumps.
2. `npm audit --omit=dev` reports zero vulnerabilities, and the two audits from
   ITEM-111 still pass (no new licence enters the graph).
3. `npm run check:types`, the vitest unit suite and `next build` pass in CI and
   locally.
4. The web image is rebuilt on the durable host and verified live: public `/` and
   `/fr` 200, a dashboard page 200 with a real session, a real completion through
   the proxy 200, and no errors in the web or proxy logs.
5. The README/guide versions, if any, still match (no version is documented as a
   contract; confirm before changing prose).

**Technical Notes**
- `npm audit fix` should perform both bumps; review the lock diff rather than
  trusting it, then run the checks above.
- Next.js patch bumps have historically changed a type or a config default: read
  the release notes for 16.4.x before merging.
- This item was captured, not planned: it outranks the feature work because the
  affected code is already live.

**Source:** `npm audit --omit=dev` in `web/`, 2026-11-05.

## Delivered (2026-11-05)

- `web/package.json`: `next` `^16.2.9` → `^16.4.5`, `zod` `^4.4.3` → `^4.6.7`.
  The lockfile moved only `next`, the ten `@next/swc-*` platform packages and
  `zod`; no other dependency changed.
- `npm audit --omit=dev`: **0 vulnerabilities** (was 1 critical + 1 high).
- Checks: `next typegen`, `tsc --noEmit`, the 80 vitest unit tests and
  `next build` pass locally (`--webpack` for the SMB workspace; the image builds
  with Turbopack on Linux). The ITEM-111 audits still pass: `cargo deny check
  licenses` ok, `node scripts/check-npm-licenses.mjs` ok.
- Live on the durable host: the web image runs Next.js 16.4.5; TLS `/healthz`,
  `/` and `/fr` return 200, `/dashboard` plus Runtime, Spend and Settings return
  200 with a minted session, a real completion through the proxy returns 200, and
  the deploy window has no web errors and no proxy panics.
- The deploy command (`up -d --build web`) also rebuilt the proxy because compose
  builds the service's dependency and the chef layer cache missed; the proxy came
  back healthy and no behaviour changed. Next time build only the image
  (`docker build -t inferon-web ./web`) and bring it up with `--no-build`.
