## ITEM-112: Hosted documentation site (free static hosting)

**As a** developer evaluating Inferon
**I want** the documentation on a stable public site with search
**So that** I can find what I need without reading the repository

**Context (2026-11-05, PO request).** The docs are Markdown files in the repository
(`README.md`, `guides/`). A visitor has to know the repo exists, and the guides are
not reachable from the product. The PO wants the documentation hosted, for free.

**Host choice — Cloudflare Pages (recommended).**
Free plan, no credit card: unlimited bandwidth and requests, 500 builds/month,
custom domain with automatic TLS, and a preview deploy per pull request. That is
the most generous free tier for a static docs site that is meant to attract traffic.

| Host | Free tier | Catch |
|---|---|---|
| **Cloudflare Pages** | static hosting, unlimited bandwidth, custom domain + TLS, PR previews | 500 builds/month (far more than we need) |
| GitHub Pages | simple, deploys from Actions | 1 GB site, ~100 GB/month soft bandwidth, public repo only |
| Read the Docs | free for open-source projects, versioned builds, search | their theme and build model; less flexible |
| Netlify | 300 build minutes/month, 100 GB bandwidth | limits are reached sooner |
| Vercel Hobby | fast Next.js path | **its terms restrict commercial use** — not usable for a for-profit docs site |
| Our own durable host | we already run Caddy there | mixes docs with the product host; a docs rebuild would touch the deployment |

Also free and worth using: **docs.rs** builds the Rust API reference automatically
when the crate is published; if the crate stays unpublished, `cargo doc` in CI does
the same job.

**Acceptance Criteria**
1. A static docs site is published at **docs.inferon.agilefederation.com** on
   Cloudflare Pages (or GitHub Pages if a second account is unwanted), with TLS and
   a redirect from the apex `/docs` path on the main site.
2. The generator is Astro Starlight or MkDocs Material — static output, built-in
   search, no runtime service. The choice and the reason are recorded in the item.
3. **One source of truth:** page content lives in `docs/` in this repository;
   `guides/*.md` is moved into it and `README.md` links to the hosted pages instead
   of to files. No page exists twice.
4. A CI job builds the site on every pull request and fails the pull request when the
   build breaks; a merge to `master` publishes it.
5. Navigation covers: Quickstart · Concepts (workspace, provider, routing and tier
   pairs, budget, output checker, pruning, datasets, evals) · Guides (ITEM-113) ·
   Reference (environment variables, API endpoints, error codes) · Troubleshooting
   (ITEM-113) · Self-hosting · Release notes.
6. Each page carries "Edit this page" (link to the repo file) and "Last verified
   <date>" so a reader can see how current it is.
7. Moving a page leaves a redirect behind; the CI link check finds broken internal
   links and old URLs.
8. Verified live: TLS 200, search returns results for "budget" and "fallback",
   mobile layout readable, and the links from `README.md` and the landing footer
   resolve.
9. Documentation keeps the plain-language standard from ITEM-110 (mean sentence
   under 25 words), checked by a script rather than by eye.

**Technical Notes**
- Static only: no new container, no new database, nothing added to the product host.
- Keep it in this repository (`docs/`) so a code change and its documentation land in
  the same pull request; the site build reuses the repo's Markdown.
- Publish `sitemap.xml` and `robots.txt`. Consider `llms.txt` so assistants can find
  the entry points.
- Cost at this scale: $0. The only recurring cost is the domain, which we already
  own.

**Source:** PO request, 2026-11-05 ("i want the documentation to be hosted
somewhere… identify a site that hosts technical documentation for free").
