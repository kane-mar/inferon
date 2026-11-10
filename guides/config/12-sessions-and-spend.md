# Spend, sessions, and why the total differs from the provider's bill

**Spend** answers "where did the money go" and **Overview** answers "how much". This page
explains what is behind both, so a number that disagrees with a provider invoice can be
explained instead of argued about.

## What a request costs here

Every trace stores:

- `prompt_tokens` and `completion_tokens` : as the provider reported them;
- `cached_prompt_tokens` : the part served from the provider's prompt cache;
- `upstream_cost_usd` : priced from our table at the model's per-million input/output
  price, with the cache-hit price for the cached part;
- `savings_usd` : the difference between what you paid and what the *frontier* model would
  have cost, split into `router`, `fallback` and `cache`.

A model with no price in the table is `unpriced`: its requests are counted and its cost is
the default row, and the product warns once rather than silently guessing. A **locally
hosted** model (`kind: local`) is costed at exactly zero , that is a fact about your own
hardware, not an unknown model.

## Sessions: what one user transaction cost

Requests are the wrong unit for an agent. Send `X-Inferon-Session-Id` (any bounded
printable id) with every call of one transaction and **Overview → Sessions** groups them:

```json
{"distribution":{"p50_cost_usd":"0.0041","p95_cost_usd":"0.0230","sessions":37},"sessions":[…]}
```

That is the number to compare against a per-conversation price, and the one that makes
"10–20 calls per transaction" visible. Requests without the header are not grouped : they
are not guessed into a session.

## Why the total can differ from the provider's invoice

| Cause | Direction | How to check |
|---|---|---|
| The provider bills in its own unit (tokens rounded, per-request minimums) | either | compare one request's `prompt_tokens` with the provider's dashboard |
| A provider-specific cache discount we do not model for that model | ours is higher | note the model and its cache in `cached_prompt_tokens` |
| Retries inside the provider (it bills the failures we never saw) | theirs is higher | provider's own usage log for the same window |
| Prompts counted differently (system messages, tool schemas) | either | the payload is captured: count it yourself |
| Time-zone and window edges | either | both sides: use the same UTC window |
| Local models | ours is zero | a `local` trace costs 0 by design |

Our number is an accounting of what each request used at our prices. It is not a copy of
the provider's invoice, and it never will be.

## Reading the breakdowns

- **By model**: the primary view. A frontier model taking a large share with small
  requests is finding 1 of the [cost audit](11-cost-audit-findings.md).
- **By provider**: which account the money left through.
- **By key**: which service spent it. One key per service makes this list meaningful.
- **By system prompt**: the same prompt hash across models, with Cached %; this is where
  [prompt caching](07-prompt-caching.md) pays or does not.
- **Transactions**: individual requests, with cost and status.

The window chip (`7`, `30`, `90` days) applies to every one of them; nothing on the page
has its own private window.

## How to confirm it worked

- **Overview → Total cost** matches the sum of the by-model table for the same window.
- A `local` model's rows show `$0` and its savings are still attributed (the counterfactual
  is reported even when the actual cost is zero).
- A trace you can name (by `x-inferon-trace-id`) has the same cost in **Transactions** as
  here.

## When to stop and ask

- A model shows `$0` and is not `local`: it is unpriced, and its cost is the default row.
- The provider's invoice is materially higher for the same window and the same models.
- Sessions are empty although your requests carry the header : check the header is sent on
  every call of the transaction; one call without it breaks the grouping.

_Last verified 2026-11-06 against the live host (master acdaffc)._
