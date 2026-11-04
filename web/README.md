# Inferon web app

The Next.js human surfaces for Inferon (ITEM-042): the landing page, the
sign-in-only dashboard and the operator console. This directory started as a copy of
`ixartz/SaaS-Boilerplate` (MIT — see `LICENSE`). Everything we do not use was
removed (Clerk, Stripe, Drizzle and its `migrations/`, Sentry, Crowdin, Checkly, the
template's `.github/workflows`), and the rest was re-skinned for Inferon.

The repository root `README.md` has the whole architecture: the `web` container
versus the Rust `proxy` data plane, and how Caddy routes between them. It also
covers the project's "Rust + bash" rule and the one exception it makes for
TypeScript in `web/`.

## Run it locally

Node ≥ 24.

```bash
npm ci
npm run dev                       # dev server (http://localhost:3000)
# or
npm run build && npm start        # production server (standalone output)
```

Point it at a running proxy with `INFERON_API_URL` (it defaults to
`http://proxy:8080` inside compose; use `http://localhost:8080` for a proxy on your
own machine). The app passes the browser's session cookie on to the Rust `/auth/*`
and `/v1/*` APIs. It never checks sessions itself.

## Checks

```bash
npm run check:types   # tsc --noEmit
npx vitest run --project unit   # unit tests (what CI runs; no browser)
npm test                        # all Vitest projects — the `ui` project needs `npx playwright install`
npm run check:i18n    # locale key parity
npm run check:deps    # knip (unused deps/exports)
npm run lint          # eslint
npm run build         # next build (standalone)
```

This repository lives on an SMB share where `npm` stalls, so develop in a local
copy:
`rsync -a web/ ~/.cache/inferon-web-sandbox/ --exclude node_modules --exclude .next`.
