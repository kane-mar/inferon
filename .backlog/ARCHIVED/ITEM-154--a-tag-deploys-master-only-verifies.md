## ITEM-154: A tag deploys; a push to master only verifies

**Requested** 2026-11-11: "CI deploys only on a tag, with pushes to master limited to
the checks."

**Why.** Every push to master rebuilt both images on the 1 vCPU host: about an hour,
and it filled the 23 GB disk twice (once taking postgres unhealthy, ITEM-153). The
concurrency group was at workflow level, so a push also queued behind a running deploy
before its checks could even start.

**Delivered**

- **Triggers**: `push` to `master` for the checks and the docs publish, `push` of a
  `v*` tag for a deploy, plus `workflow_dispatch`.
- **`deploy`** is `if: startsWith(github.ref, 'refs/tags/v')` and `needs` every check
  (`test`, `web`, `licences`, `docs`, `docs-site`, `self-hosted`, `embedder`), so a tag
  releases only from a green tree.
- **Concurrency moved from the workflow to the jobs that need it**: `durable-deploy`
  for the deploy (never cancelled), `pages` for the docs publish. A push to master no
  longer queues behind an hour-long deploy.
- **Both deploy paths prune the build cache** after the image is built, so a deploy
  leaves the disk where it found it.

**The first tag deploy failed, and the reason matters.** The deploy step was one long
double-quoted ssh argument with line continuations; appending the prune broke its
quoting, and the runner reported `unexpected EOF while looking for matching quote`
before running a single remote command. It is now a **quoted heredoc** (`<<'REMOTE'`)
carrying a plain script, so nesting cannot break it. The environment file is written
with `printf %q` — a webhook URL with a query string, or a login list with a comma,
survives being sourced again — and shipped with `scp` + `install -m 600`. Both were
verified locally by round-tripping a value containing a space, an `&` and a comma.

**Verified live**

- Push to master (`b9c1ac5`): `test, web, licences, docs, docs-site, self-hosted,
  embedder, pages` all **success**, `deploy` **skipped**.
- Tag `v0.1.2`: the same checks, then `deploy` **success** through health, performance
  and public-landing, ending with `env.local` at `600` on the host, no deploy secret in
  `ps`, all five services healthy and **6.9 GB free** (the prune worked).
- Public: `healthz` 200, a real completion 200, the docs site 200.

**Note on the tags**: `v0.1.1` carries the broken deploy step. Its source is fine; only
the release pipeline of that tag is not. `v0.1.2` is the first tag that deploys
cleanly.

**Source:** PO request, 2026-11-11.
