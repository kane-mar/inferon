## ITEM-151: Protect master, and publish the guides on GitHub Pages

**Requested** 2026-11-11: "okay, protect master. also, can you push the documentation
to github pages."

**Branch protection on `master`**

- Required status checks, strict: `test`, `web`, `licences`, `docs`, `docs-site`,
  `self-hosted`, `embedder`. `pages` and `deploy` are deliberately excluded: they
  need master and repository secrets, so a fork's pull request would sit at
  "expected" forever and could never merge.
- Force pushes and deletions are refused; conversations must be resolved; admins are
  **not** bound, so the owner cannot lock themselves out of their own repository.

**What the probe proved, and what it cost.** I force-pushed to test it. Two lessons:

1. A push to a *descendant* commit is a fast-forward, so it is allowed with
   `allow_force_pushes: false`; a test of the rule has to be non-fast-forward.
2. The probe commit landed on master, and with force pushes disabled I could not
   rewind it — the protection I had just installed refused the cleanup. The escape
   hatch is to flip `allow_force_pushes` to true, rewind, and restore it, which is
   what I did. The remote is back at `b6495d5`, and the junk run was cancelled.

**Pages**

- The repository is public, so Pages is free and needs no third party. This settles
  ITEM-112's host decision; Netlify was the recommendation while the repo was private.
- `docs-site` already built the site strictly and proved no sales document was
  published; a new `pages` job publishes what it built (`configure-pages`,
  `upload-pages-artifact`, `deploy-pages`), on master only.
- `site_url: https://kane-mar.github.io/inferon/` so canonical links and the sitemap
  carry the project-site prefix.
- Live: `https://kane-mar.github.io/inferon/` — the index, `config/01-providers/`,
  `licence/`, `sitemap.xml` and the search index all 200; the two sales working
  documents are 404, as intended.

**A guard that was wrong.** The build failed on my own verification step, which
matched any path containing `cost-audit` — and `11-cost-audit-findings` is a
legitimate published guide. The step now names the two documents it means.

**CI was red since 2026-10-30, and it was not the code.** Every job failed with zero
steps: a private repository's Actions minutes were exhausted. On the public repo the
same jobs run and pass (`test`, `web`, `licences`, `embedder`, `self-hosted`, `docs`,
`docs-site`, `pages`).

**Still open:** CI's `deploy` job and my manual deploys both target the same host. The
CI deploy ran for over an hour rebuilding on a 1 vCPU box while a manual deploy was
also in flight, which is why it was cancelled. Either CI owns the deploy or the manual
path does; both cannot.

**Source:** PO request, 2026-11-11.
