# Inferon web app

The Next.js human surfaces for Inferon (ITEM-042): rebranded landing, the
session-gated dashboard, and the operator console. This directory started as a
vendor of `ixartz/SaaS-Boilerplate` (MIT — see `LICENSE`) and was then stripped
of everything we don't use (Clerk, Stripe, Drizzle + its `migrations/`, Sentry,
Crowdin, Checkly, the template's `.github/workflows`) and re-skinned for Inferon.

See the repository root `README.md` for the full architecture (the `web`
container vs. the Rust `proxy` data plane, Caddy routing) and the project's
"Rust + bash" decision with its scoped TypeScript exception for `web/`.

## Run locally

Node ≥ 24.

```bash
npm ci
npm run dev                       # dev server (http://localhost:3000)
# or
npm run build && npm start        # production server (standalone output)
```

Point it at a running proxy: `INFERON_API_URL` (defaults to
`http://proxy:8080` inside compose; use `http://localhost:8080` for a local
proxy). The app forwards the browser's session cookie to the Rust `/auth/*` and
`/v1/*` APIs — it never verifies sessions itself.

## Checks

```bash
npm run check:types   # tsc --noEmit
npm test              # Vitest (unit project)
npm run check:i18n    # locale key parity
npm run check:deps    # knip (unused deps/exports)
npm run lint          # eslint
npm run build         # next build (standalone)
```

Note: this repository lives on an SMB share where `npm` stalls — develop in a
local copy: `rsync -a web/ ~/.cache/inferon-web-sandbox/ --exclude node_modules --exclude .next`.
