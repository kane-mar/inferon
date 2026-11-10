# Licence: what you may do, and what hosting asks of you

Inferon is open source under **AGPL-3.0-only**. That is the whole of the code
licence, and it is not a trial: there is no time limit, no user cap and no feature
held back. This page says what that means in practice, so you can decide without a
lawyer for the common cases.

The full text is the `LICENSE` file in the repository. Where this page and the
licence text disagree, the licence text wins.

## What you may do without asking

- Read the code, and check it does what this documentation says.
- Run it for yourself or inside your own organisation.
- Change it. Fork it and take it in a different direction.
- Host it for other people, including as a paid service, as long as you meet the
  network condition below.

Nothing here needs a purchase order or an email to us.

## What hosting it for others asks of you

If you **modify** Inferon and let other people use that modified version over a
network, you must offer those users the source of your version, under the same
licence. This is section 13 of the AGPL, and it is the only condition that
reaches a service rather than a download.

Two honest consequences:

- A **modified** version cannot be kept closed. If your users reach your version
  over the network, they are entitled to its source.
- An **unmodified** copy carries no such obligation. Someone may run stock
  Inferon and owe the project nothing. We say so here rather than let the licence
  sound stronger than it is.

## A commercial licence, and what it adds

Some organisations cannot accept an AGPL dependency at all, whatever the code
does. For those, a commercial licence is available for the same build. Email
<kane@inferon.dev>.

The commercial licence sells **terms**, not features:

| | AGPL-3.0-only | Commercial licence |
|---|---|---|
| Read, change and run the code | yes | yes |
| Self-host for your own organisation | yes | yes |
| An organisation with a policy against AGPL | cannot | can |
| Warranty, indemnity, support level | no | written per agreement |
| Same features as the free build |: | yes, identical |

There is no paid version of the product with more in it. If a page in these guides
describes it, the free build has it.

## What we will not do

We will not take a release that you already have under the AGPL and relicense it.
The licence on a published version stays as published. That commitment is the
reason this page exists: several projects have changed their terms under their
users, and the cost of that is trust you cannot buy back.

## The name is not the code

The licence covers the code. It does not give you the right to use the Inferon name
or logo for your own product. A fork may say it is "based on Inferon". It may not
present itself as Inferon. See `TRADEMARKS.md` in the repository.

## This documentation

These guides are published under **CC BY-4.0**: quote them, translate them, put
them in your own docs, credit the source. The code they describe stays AGPL.

## How to tell what you are running

Every image is built from a repository tag, and the `LICENSE`, `LICENSES.md` and
`TRADEMARKS.md` files ship inside it. To check a self-hosted install:

```bash
docker compose exec proxy cat /LICENSE | head -3
```

That prints the AGPL header, which is the same text for every version.

## Dependencies

Inferon uses permissive dependencies only, and both audits run in CI: Rust crates
against an allow-list in `deny.toml`, and npm packages against
`scripts/check-npm-licenses.mjs`. The `LICENSES.md` file in the repository lists
every licence and why it is allowed. If you need to audit a self-hosted install,
the same two commands run there.

_Last verified 2026-11-07 by `scripts/check-guides.py` (master 963f553)._
