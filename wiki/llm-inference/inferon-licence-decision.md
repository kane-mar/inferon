# Inferon Licence Decision: AGPL-3.0-only with a CLA (2026-11-05)

> Raw: [2026-11-05 licence direction](../../raw/llm-inference/2026-11-05-inferon-licence-direction.md)
> Updated: 2026-11-05
> Purpose: record why the code is AGPL-3.0-only, what that obliges, and the trigger
> that would make us revisit it.

## Decision

Inferon's code is licensed **AGPL-3.0-only**, with a Contributor Licence Agreement
that lets the owner also license the same code commercially.

The commercial path (backlog item 114) sells **terms**, not extra code: relief from
the AGPL, warranty, indemnity, and a service-level agreement for the same Docker
image. It is gated on demand and is not being built yet.

## Why AGPL rather than the alternatives

- **MIT / Apache-2.0** gives the best adoption, but someone can host a closed fork
  and sell it. There is nothing left to license.
- **BSL 1.1 / FSL** stop that, but they are source-available, not open source. The
  traction plan depends on being able to say "open source".
- **Open core** (Apache-2.0 core, proprietary modules) keeps the core adoptable but
  only monetises the add-ons, and the core remains free to host.
- **AGPL** stays genuine OSI open source, makes a hosted competitor publish its
  changes, and creates the thing a commercial licence sells. It is the only option
  that satisfies all four columns at once.

## What the licence obliges

Running, changing, and self-hosting Inferon is free. If a modified version is
offered to other people over a network, its source must be offered to them under
the same licence. There is no "may not compete" clause: competition is constrained
by copyleft and by trademark, not by an extra restriction.

Shipping a Docker image counts as distribution, so the images carry the licence
text and the corresponding source is this repository at the matching release tag.

## Why the CLA comes first

Dual licensing only works if the seller owns, or holds a broad enough licence grant
to, every line of the code. The CLA grants that right (including the right to
relicense) without taking the contributor's copyright. It therefore has to be in
place **before** the first outside contribution merges, not after. The history is
currently a single author, so there is nothing to back-fill.

The same reasoning makes `web/`'s MIT-derived template parts a notice question, not
a licensing conflict: MIT permits sublicensing, and the notice is kept in
`web/THIRD-PARTY-NOTICES.md`.

## Trigger to revisit

The cost we accepted is that AGPL sits on many corporate deny-lists. Revisit the
decision, and take the recorded fallback (**Apache-2.0 core plus proprietary
enterprise modules**), when the friction is demonstrably the bigger obstacle:

- a prospect with real budget stops on the licence, or
- several developers say the licence blocks adoption, or
- procurement cannot approve AGPL even with a commercial option.

Revisit is a decision about the whole repository, not a per-file change; it needs a
new item, a fresh CLA review, and a new licence audit.

## Where it is implemented

`LICENSE`, `LICENSES.md`, `CLA.md`, `CONTRIBUTING.md`, `TRADEMARKS.md`, the licence
fields in `Cargo.toml` and `web/package.json`, the allow-lists in `deny.toml` and
`scripts/check-npm-licenses.mjs`, the licence job in CI, and the `COPY` lines in
both Dockerfiles.
