# Licence direction for Inferon (2026-11-05)

> Source: PO request, supplied in the pi session on 2026-11-05: "identify an
> appropriate open source license… consistent with the ultimate goal of the
> for-profit corp".
> Collected: 2026-11-05
> Published: 2026-11-05
> Status: source record (immutable). The compiled decision lives in
> `wiki/llm-inference/inferon-licence-decision.md`.

## The request

Identify an appropriate open-source licence for Inferon that is consistent with the
end goal: a for-profit company that sells (a) the hosted service and (b) a licence
of the Docker image. The repository had no root `LICENSE`, so it was legally "all
rights reserved" and the open-source traction plan had no legal basis.

## The options considered

| Option | Adoption | Stops a competitor hosting it | Supports selling an image licence | Honest "open source" claim |
|---|---|---|---|---|
| MIT / Apache-2.0 | best | no — anyone may host and sell a fork | no lever | yes |
| AGPL-3.0-only | good, but on many corporate deny-lists | yes — a hosted fork must publish its source | yes — a commercial licence is the exception to the copyleft | yes (OSI) |
| BSL 1.1 / FSL | fair | yes | yes | no — source-available, not open source |
| Apache-2.0 + proprietary enterprise modules (open core) | best for the core | partly | yes, for the add-ons only | yes |

## The decision

AGPL-3.0-only for the core, with a Contributor Licence Agreement so the owner can
also license the code commercially.

Reasoning, as recorded in the backlog refinement log:

- MIT/Apache-2.0 gives the best adoption but hands a competitor the right to host a
  closed fork and leaves nothing to sell.
- BSL/FSL protect the business but cannot be called open source, which kills the
  stated traction mechanism.
- Open core keeps the core adoptable but only monetises the add-ons.
- AGPL is the only option that keeps the open-source claim, makes a hosted
  competitor publish their changes, and creates the thing a commercial licence
  sells: relief from copyleft, plus warranty and indemnity.

Cost accepted: AGPL is on many corporate deny-lists, which is friction for exactly
the developer audience the project wants. The recorded fallback is Apache-2.0 plus
proprietary enterprise modules if that friction turns out to be the bigger
obstacle.

The CLA must land before the first outside contribution is merged, because dual
licensing needs the right to relicense every contribution. The repository history is
currently a single author, so no back-fill is needed.

Trademark rights are kept separate from the code licence: the licence grants no
right to the name "Inferon" or the logo.

Nothing in the licence or the trademark policy adds a "may not compete"
restriction: that would stop it being open source and would sink the traction plan.
Competition is constrained by copyleft and by the trademark, not by an extra clause.

## What was out of scope

Selling a commercial licence now (backlog item 114). That waits for a customer: the
first "we cannot use AGPL", or ten active external workspaces, or a procurement
conversation that stops on the licence.
