# Prompt caching: why the audit says "not cached"

Caching here is the **provider's** prompt cache, not ours. When you send the same long
prefix again, a system prompt, a document, a tool schema, the provider can serve that
part from cache at a fraction of the input price. Inferon reports what it was charged and
what that saved; it cannot make a prompt cacheable.

## What the numbers mean

- **Cached %** per prompt (**Spend → Spend by system prompt**): the share of prompt
  tokens served from the provider's cache.
- **Cache savings** (**Overview**, and per model in **Spend**): the difference between the
  cached and uncached price for those tokens, in dollars.
- Both come from the provider's own `usage` block (`prompt_cache_hit_tokens` /
  `prompt_tokens_details.cached_tokens`). If the provider does not report it, we report
  nothing rather than estimating.

## Why a prompt is not cached

In order, the usual reasons:

1. **The prefix is too short.** Providers cache from a few hundred tokens up. A 40-token
   system prompt is below every provider's minimum.
2. **The prefix is not actually stable.** Caching matches a *prefix*, from the first
   token. One changing word at the top of the system prompt : a timestamp, a user name, a
   request id : invalidates everything after it.
3. **The prefix is below the changing part.** Interpolating the user's question into the
   system prompt moves the whole cache boundary to the start. Put stable text first and
   variable text in the messages.
4. **The cache expired.** Provider caches last minutes to hours; traffic that arrives
   once an hour will rarely hit.
5. **The request went to a different provider or model.** Caches are per provider and per
   model. A fallback step, or load balancing across two providers, spreads the same prompt
   over two caches and halves the hit rate.

The audit's **big system prompt that is never cached** finding ([reading the cost
audit](11-cost-audit-findings.md)) is exactly reason 1–3, priced in dollars.

## What to change

- **Move stable text to the front.** A fixed system prompt, then a fixed tools block, then
  the conversation.
- **Keep the prompt byte-identical between calls.** No timestamps, no ordering that
  changes, no pretty-printing differences.
- **Group traffic that shares a prefix** onto the same model : cache affinity keeps a warm
  request on the model it was warm on, and the trace records it as
  `"cache_affinity": "kept"` when it does.
- **Do not chase caching on short prompts.** Below the provider's minimum there is nothing
  to win.

## How to confirm it worked

- **Spend → Spend by system prompt** shows the prompt's Cached % rising for the same
  system prompt hash.
- **Overview → Cache savings** grows, and the per-request answer carries no error.
- The trace holds `cached_prompt_tokens` and `cache_savings_usd`; a request that hit cache
  shows a non-zero count.

## When to stop and ask

- Cached % is 0 for a long, stable system prompt over hundreds of requests : check the
  provider actually returns the cache fields for that model.
- Savings are reported but the provider's own bill does not reflect them : the bill is the
  authority; our number is derived from `usage`.
- Two workspaces share a prompt and report different hit rates : the provider caches per
  account, not per workspace.

_Last verified 2026-11-06 against the live host (master acdaffc)._
