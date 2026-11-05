## ITEM-111: Licence, CLA and trademark (AGPL-3.0-only + dual-licensing readiness)

**As the** project owner
**I want** a licence in place now that stays honest about being open source, and still
lets me sell services and a commercial licence of the image later
**So that** the code I publish for traction today does not become a legal wall in
front of the company I intend to build

**Context (2026-11-05, PO request).** The repository has **no root `LICENSE`** and no
`license` field in `Cargo.toml` or `web/package.json`, so today it is "all rights
reserved" by default: nobody may legally use, copy or host it, which defeats the
plan to win traction as an open-source project. The PO's end state is a for-profit
company selling (a) the hosted service and (b) a licence of the Docker image, with
open source as the on-ramp.

**Recommendation — AGPL-3.0-only, with a CLA so we can dual-license.**

| Option | Adoption | Protects against a competitor hosting it | Supports selling an image licence | Honest "open source" claim |
|---|---|---|---|---|
| MIT / Apache-2.0 | best | no — anyone may host and sell a fork | no lever (nothing to licence) | yes |
| **AGPL-3.0-only** | good, but on many corporate deny-lists | **yes** — a hosted fork must publish its source | **yes** — commercial licence is the exception to the copyleft | **yes (OSI)** |
| BSL 1.1 / FSL | fair | yes | yes | no — source-available, not open source |
| Apache-2.0 + proprietary enterprise modules (open core) | best for the core | partly | yes, for the add-ons only | yes |

AGPL fits the stated plan: it is genuine OSI open source (so the publicity story
works), the network copyleft stops a larger company from hosting our code and
closing it, and it is the licence that makes "sell a commercial licence of the same
image" meaningful — the buyer is paying for relief from copyleft, plus warranty and
indemnity. Choosing it costs us some enterprise adoption; the fallback, if that
friction turns out to be the bigger obstacle, is Apache-2.0 plus proprietary
enterprise modules, which gives up the licence-resale lever but keeps the core
adoptable.

**Acceptance Criteria**
1. `LICENSE` at the repo root holds the AGPL-3.0-only text; `license = "AGPL-3.0-only"`
   in `Cargo.toml` and the same in `web/package.json`; a plain-language "Licence"
   section in `README.md` states what you may do and when to talk to us about a
   commercial licence.
2. A third-party licence audit passes: every Rust crate and npm package is
   AGPL-3.0-compatible; the allow-list and any exceptions live in `LICENSES.md`; the
   ONNX Runtime and BGE-micro model artifacts are described as downloads, not as
   vendored code.
3. CI enforces the audit (`cargo deny check licenses` plus an npm licence scan), so a
   new dependency with an incompatible licence fails the build.
4. `CLA.md` is in place and grants the project owner the right to relicense a
   contribution under other terms, so dual licensing stays possible. `CONTRIBUTING.md`
   says the CLA is required before a contribution merges. The history is currently a
   single author, so no back-fill is needed — the item records that.
5. `TRADEMARKS.md` states that the code licence grants no right to the name
   "Inferon" or the logo, and says what a fork may call itself.
6. The reasoning above (why AGPL rather than Apache-2.0, BSL or open core) is
   recorded in the wiki, with the trigger that would make us revisit it.
7. Nothing in the licence or README adds a "may not compete" restriction: that would
   stop it being open source and would sink the traction plan. Competition is
   constrained by copyleft and by the trademark, not by an extra clause.

**Technical Notes**
- Dual licensing only works if the seller owns, or holds a broad enough licence
  grant to, every line of code. That is why the CLA must land **before** the first
  outside contribution is merged, not after.
- AGPL-3.0 is compatible with Apache-2.0 (one-way, into AGPL) and MIT. The risk is
  in transitive crates and npm packages — check them, do not assume.
- Distribution of the Docker image is distribution of AGPL code: the image must ship
  the licence text and an offer of the corresponding source (the repo link is
  enough while the running code matches a public tag).
- Also covered by the root licence and worth stating explicitly: `db/init/*.sql`,
  `scripts/`, `web/`, `guides/`, `docs/`.
- Later item, gated on traction: **ITEM-114** (commercial licence and paid image).

**Source:** PO request, 2026-11-05 ("identify an appropriate open source license…
consistent with the ultimate goal of the for-profit corp").
