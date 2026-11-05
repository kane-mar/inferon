## ITEM-112: Hosted documentation site (free static hosting)

**As a** developer evaluating Inferon
**I want** the documentation on a stable public site with search
**So that** I can find what I need without reading the repository

**Context (2026-11-05, PO request).** The docs are Markdown files in the repository
(`README.md`, `guides/`). A visitor has to know the repo exists, and the guides are
not reachable from the product. The PO wants the documentation hosted, for free.

**Host choice — awaiting PO confirmation (2026-11-05).** Cloudflare was removed at
the PO's request, so the choice is open again. The constraints that filter it:

- **$0**, custom domain with automatic TLS;
- the repository `kane-mar/inferon` is **private** today (GitHub returns 404
  unauthenticated), and Inferon is a commercial product (AGPL today, a paid tier is
  ITEM-114), so "free for open source only" plans are not automatically safe;
- search is required, PR previews are wanted, nothing may run on the product host.

| Candidate | $0? | Private repo? | Commercial use? | Catch |
|---|---|---|---|---|
| **Netlify (Free)** — *recommended* | yes | yes | yes, their terms allow it | 100 GB bandwidth and 300 build minutes/month (docs build in seconds); PR previews included |
| GitHub Pages (via Actions) | yes | **no** — free Pages needs a public repo (private needs a paid plan) | ToS says Pages is not for running your business; a product's docs site is the grey edge, not the intent | repo must go public first (reasonable for AGPL, but a decision, not a detail); 1 GB site, 100 GB/month soft bandwidth, 10 builds/hour only if not built by Actions |
| Read the Docs Community | yes | no (builds from a public VCS URL) | **free only for open-source projects** | best search and versioning of the four; their theme and build model, less control |
| Render (static site) | yes | yes | yes | 100 GB/month; no PR previews on the free plan |
| Vercel Hobby | yes | yes | **no** — its terms restrict commercial use | rejected |
| Cloudflare Pages | yes | yes | yes | **removed at the PO's request (2026-11-05)** |
| This durable host | yes | n/a | yes | mixes the docs with the product deployment; a docs rebuild would touch the host |

**Recommendation:** Netlify Free — it is the only candidate that is $0, works with a
private repository, allows commercial use and gives PR previews. **Second:** make the
repository public (it is AGPL; the goal is external developers anyway) and use GitHub
Pages, which keeps the site in the same place as the code. **Revisit** Read the Docs
if versioned docs (`v1`, `latest`) become a requirement.

The generator decision stands either way (AC2), so the site can be built and previewed
before any host is confirmed.

**Acceptance Criteria**
1. A static docs site is published at **docs.inferon.agilefederation.com** on the
   host the PO confirms (see the table above; Netlify is the recommendation), with TLS
   and a redirect from the apex `/docs` path on the main site.
2. **Decision 2026-11-05: MkDocs Material with `docs_dir: guides`.** Static output,
   built-in search, no runtime service. `guides/` *is* the docs directory, so no page
   moves and no second copy appears; `guides/README.md` becomes the site home and the
   site is configured with `exclude_docs` for the two sales working documents
   (`inferon-cost-audit.md`, `inferon-step2-materials.md`). Starlight was rejected
   because it would require relocating every page into `src/content/docs/`.
3. **One source of truth:** the Markdown in `guides/` is the site; `README.md` links
   to the index (`guides/README.md`) and to the hosted pages once they exist.
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
- The host is a PO decision (2026-11-05); the `mkdocs.yml` + build workflow is not,
  so the site can be built locally and the output attached to any of the candidates.
- Keep it in this repository (`docs/`) so a code change and its documentation land in
  the same pull request; the site build reuses the repo's Markdown.
- Publish `sitemap.xml` and `robots.txt`. Consider `llms.txt` so assistants can find
  the entry points.
- Cost at this scale: $0. The only recurring cost is the domain, which we already
  own.

**Source:** PO request, 2026-11-05 ("i want the documentation to be hosted
somewhere… identify a site that hosts technical documentation for free").
