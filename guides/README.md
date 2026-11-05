# Inferon guides

Start here if you have just signed in, or if something has stopped working. Every
page is a task: it names the dashboard page or the command you need, shows what a
good result looks like, and says what to check when you get something else.

## Getting started

| Page | What it gets you |
|---|---|
| [Your first request](using/first-request.md) | Sign in, create a key, add a provider and see the request in the dashboard. About five minutes. |
| [Move an existing SDK to Inferon](using/move-an-sdk.md) | Change two lines in Python, Node or curl. Nothing else in your code moves. |

## Troubleshooting

One page per symptom. Same shape every time: what you see, what it means, the fix, how
to confirm it, and when to stop and ask.

| Page | Answers |
|---|---|
| [401 or 403: the key is not accepted](troubleshooting/401-403.md) | Missing, malformed or revoked key; a member hitting an owner-only action |
| [no_provider](troubleshooting/no-provider.md) | The workspace has nowhere to send the request — the most common first failure |
| [429: our limit, or the provider's](troubleshooting/too-many-requests.md) | Our per-key limit versus the provider's own refusal |
| [5xx and the fallback chain](troubleshooting/upstream-5xx.md) | A provider failure, a connection failure of ours, and what an exhausted chain returns |

## Running it yourself

| Page | What it gets you |
|---|---|
| [Inferon on your own network](inferon-self-hosted.md) | Install the same images on your own Docker host: install, workspace, local model, backup, upgrade, air-gapped hosts, support boundary. |

## How to read a page

- **Commands can be copied as they are.** They were run against the live host on the
  date in the footer. Where a value is yours, it is written as `$INFERON_API_KEY` or
  `sk-inferon-…` so you can substitute it.
- **Every page ends with `Last verified`.** The date, where it was run and the code
  version say how old the page is. If it is older than the release you run, treat the
  commands as a starting point and check the page named under "If it did not work".
- **The product calls a workspace a *tenant* in some places.** The dashboard says
  workspace; endpoints, tables and `seed-tenant.sh` say tenant. They are the same
  thing.
- **Jargon is defined once.** The plain term comes first, the exact word in brackets,
  so you can match it to a log line.

## For maintainers

Two rules keep these pages from rotting:

1. **Re-verify, then move the footer.** Run the commands on the live host or a local
   stack, then update `_Last verified <date> against <where> (master <sha>)._` to the
   commit you tested against. Do not edit the footer without running the page.
2. **Run the checker.** `python3 scripts/check-guides.py` fails on a broken relative
   link, a missing footer, a troubleshooting page without its five sections, and a
   page whose mean sentence runs past 25 words. CI runs it on every push.
3. **Add the page to `mkdocs.yml`.** The nav at the repository root is the site's table
   of contents, and the build fails when a nav entry points at no file, or a page is
   missing from it. The site is built with `mkdocs build --strict` (Python 3.12,
   `pip install -r requirements-docs.txt`); CI builds it on every push and keeps the
   output as an artifact. `guides/` is the docs directory, so nothing is copied.

Not published: `inferon-cost-audit.md` and `inferon-step2-materials.md` are sales and
outreach working documents. They stay in the repository and out of the site, and no
page here links to them.

_Last verified 2026-11-05 by `scripts/check-guides.py` (master 5f97dd7)._

