# Reading the cost audit's six findings

The **Cost audit** (`/audit`, and its public share link) is the one-page money story: six
findings, each with a number from *your* traffic and the control that changes it. Nothing
in it is a projection — every figure is read from traces you already have.

## Finding 1 — A model that is too big for the job

*Expensive models answering tiny requests that a cheaper model would answer just as well.*

Detected when a frontier model with a cheaper partner answers a request with **fewer than
100 prompt tokens and fewer than 50 completion tokens**. These are classification and
extraction calls.

**What to do:** add the pair ([models and cheaper pairs](02-models-and-tier-pairs.md)) and
turn on the router ([the router and its threshold](04-auto-router-and-threshold.md)). The
finding's number is what those requests would cost at the cheap model's price.

## Finding 2 — Wrong model for the job

*Expensive models used on plain text, with no tools, schema or structured output.*

Detected when a frontier model with a cheaper partner answers plain text with no
`tools`/`functions` and no `response_format`.

**What to do:** the same pair and router, or move that prompt family to the cheaper model
in your own code. This is usually the largest single number on the page.

## Finding 3 — A system prompt that is never cached

*The same long prompt is sent again and again, but the provider cache is not saving you
anything.*

Detected when a system prompt is seen repeatedly over 10 minutes and the provider reports
no cached tokens for it.

**What to do:** [prompt caching](07-prompt-caching.md) — stable text first, no timestamps,
same model. The finding is priced as the difference between the cached and uncached input
price for the tokens that could have been cached.

## Finding 4 — Money already saved

*What the router and the fallback chain saved, compared with sending everything to the
expensive model.*

This is not a saving to chase; it is the record of what the product already did. It comes
from the counterfactual cost stored on each trace (`savings_usd`), split by kind
(`router`, `fallback`, `cache`).

**What to do:** if this number stays at zero while findings 1 and 2 are large, the router
is off or the pairs do not apply — start there.

## Finding 5 — What caused the bill

*Which model, provider and key drove your spend.*

The three largest contributors for the window, with their cost. It answers "who spent
this" rather than "how could this be cheaper".

**What to do:** with one key per service ([budgets and rate limits](05-budgets-and-rate-limits.md))
this list becomes an owner list. A key you cannot name is the finding.

## Finding 6 — Quality and failures

*Requests you paid for that came back empty, refused, cut short or failed — and why.*

Four numbers and the most common cause: the truncation rate, the checker fallback rate,
the error rate, and the top error class in words
([what each cause means](../troubleshooting/401-403.md) explains the classes).

**What to do:** a rising truncation rate is a `max_tokens` problem
([empty, refused or cut short](../troubleshooting/empty-refused-truncated.md)); a rising
error rate is a provider problem; a checker fallback rate that is not near zero is a cheap
model that cannot hold your schema.

## Sharing the audit

**Cost audit → Share** creates a read-only link with a token. It shows the same six
findings for a fixed window and nothing else: no prompts, no keys, no settings. Revoke it
and the link dies immediately.

## When to stop and ask

- A finding is zero although the traffic is obviously there — check the window and whether
  the traffic is `local` (locally hosted models are deliberately costed at zero).
- The dollar figures do not match your provider's bill: this page prices *our* evaluation
  of usage, and a provider invoice can differ (cache accounting, rounding, minimums).
- The audit shows a share link you did not create — revoke it and change the workspace's
  keys.

_Last verified 2026-11-06 against the live host (master acdaffc)._
