# Licences

Inferon is open source under the **GNU Affero General Public License, version 3
only** (`AGPL-3.0-only`). The full text is in [`LICENSE`](LICENSE).

Copyright (C) 2026 Kane Mar.

## What the licence lets you do

You may run Inferon, read the code, change it, and host it for your own users,
including inside a company. If you run a modified version and let other people use
it over a network, you must offer them the source of your modified version under
the same licence.

If your organisation cannot accept the AGPL, a commercial licence is available.
See the "Licence" section of [`README.md`](README.md).

## What each part of the repository is under

| Part | Licence | Notice |
|---|---|---|
| Everything in this repository, except the parts listed below | AGPL-3.0-only | [`LICENSE`](LICENSE) |
| Parts of `web/` derived from the `ixartz/SaaS-Boilerplate` template | MIT | [`web/THIRD-PARTY-NOTICES.md`](web/THIRD-PARTY-NOTICES.md) |
| Rust dependencies (crates.io) | Permissive licences only | allow-list in [`deny.toml`](deny.toml) |
| npm dependencies | Permissive and weak-copyleft licences | allow-list in [`scripts/check-npm-licenses.mjs`](scripts/check-npm-licenses.mjs) |
| ONNX Runtime 1.19.2 | MIT | Downloaded by `Dockerfile` at build time; not vendored in this repository |
| BGE-micro-v2 model | MIT | Downloaded by `Dockerfile` at build time; not vendored in this repository |

The MIT notice for the template is kept because MIT requires its copyright and
permission notice to travel with substantial portions of the code. MIT allows
sublicensing, so the combined work is distributed under AGPL-3.0-only.

## Dependency audit

Both audits run in CI on every push (the `licences` job). A new dependency with a
licence that is not on the allow-list fails the build.

```bash
# Rust: every crate in the graph, with all features, for Linux and macOS
cargo install cargo-deny --locked   # once
cargo deny check licenses

# npm: every package in web/package-lock.json
node scripts/check-npm-licenses.mjs
```

### Rust crates

325 crates.io packages in the Linux and macOS graphs (all features, including
`embedder`). Every licence is permissive. The allow-list is in `deny.toml`:

`MIT`, `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `BSD-2-Clause`,
`BSD-3-Clause`, `ISC`, `Unicode-3.0`, `Zlib`, `Unlicense`, `CC0-1.0`, `MIT-0`,
`BSL-1.0`.

Why these are compatible with AGPL-3.0-only: all of them grant the rights the AGPL
needs (use, modify, distribute) and add no field-of-use restriction. `Unicode-3.0`
and `CC0-1.0` are data licences with no copyleft. `BSL-1.0` is the Boost Software
Licence (permissive) — not the Business Source Licence.

The `MIT OR Apache-2.0 OR LGPL-2.1-or-later` expression that appears in a few
crates passes on its MIT arm, so LGPL is not needed on the allow-list.

### npm packages

947 entries in `web/package-lock.json` (the lockfile lists optional binaries for
every platform, which is why the count is higher than a Linux install). The
allow-list is in `scripts/check-npm-licenses.mjs`. Most are MIT, Apache-2.0, ISC or
BSD. The rest, and why each is acceptable:

| Licence | Packages | Why it is acceptable |
|---|---|---|
| `MPL-2.0` | `lightningcss` (CSS parser used by Tailwind) | File-level copyleft. MPL-2.0 is compatible with the GPL family; we do not modify those files, and if we did, the changes to them would stay MPL-2.0 |
| `LGPL-3.0-or-later` | `@img/sharp-libvips-*` (native image libraries) | Used as separate, dynamically loaded libraries. LGPL-3.0 permits use under GPL-3.0, and AGPL-3.0 is compatible with GPL-3.0 |
| `CC-BY-4.0` | `caniuse-lite` (browser support data) | Data, not code. Attribution is kept in the package metadata |
| `CC-BY-3.0` | `spdx-exceptions` (licence exception list) | Data, not code |
| `BlueOak-1.0.0` | `glob`, `rimraf`, `minimatch`, `lru-cache`, `minipass`, `path-scurry` | Permissive, OSI-approved, no conditions beyond keeping the notice |
| `0BSD` | `tslib` | Permissive, no attribution required |
| `CC0-1.0` | `mdn-data`, `spdx-license-ids` | Public-domain dedication |
| `Python-2.0` | `argparse` (JavaScript port) | Permissive |
| `WTFPL` | `opener` | Permissive |
| `Public Domain` | `jsonify` | Public-domain dedication by the author (no SPDX identifier) |

Two packages need a per-package exception, recorded in the script:

| Package | Declared licence | Why it is an exception |
|---|---|---|
| `format` | MIT | Declares MIT in the legacy `licenses[]` field; the published tarball has no `LICENSE` file |
| `jsonify` | Public Domain | "Public Domain" is not an SPDX identifier; the author's package declares it as a dedication |

## Docker images

The AGPL treats handing someone a container image as distribution. Both images
therefore carry the licence text (`/LICENSE` in the proxy image, `/app/LICENSE` in
the web image), and `web/THIRD-PARTY-NOTICES.md` travels with the web image. The
corresponding source is this repository at the release tag that matches the
running image.

## Changing this file

Add a row when a dependency's licence is not covered by an allow-list. Add the
licence to the allow-list only when it is compatible with AGPL-3.0-only, and write
the reason in the table above. `scripts/check-npm-licenses.mjs` refuses an
exception whose licence is not itself acceptable.
