# Models, cheaper pairs and what "expensive" means here

Inferon does not decide what a model costs. It prices each request from a table and
uses your settings to decide which model serves it. This page is about that second
part.

## Expensive and cheap, in this product

- **Expensive (frontier)**: the model your request asks for. It is the default
  answer, and nothing changes it unless you turn something on.
- **Cheap**: the model a request can be moved to when the work looks simple. A
  **pair** names both: `expensive → cheap`.

Two pairs exist on every workspace, and you never have to configure them:

| Expensive | Cheap |
|---|---|
| `gpt-4o` | `gpt-4o-mini` |
| `deepseek-v4-pro` | `deepseek-flash` |

`GET /v1/dashboard/api/routing` returns your own pairs : empty means the built-in table
above is what applies:

```json
{"tier_pairs":[],"fallbacks":[]}
```

## Add a pair of your own

**Routing → Cheaper model pairs** → choose the expensive model, choose the cheap one,
press **Add pair**. Your pairs win over the built-in ones for the same expensive model.
Each row is returned with a verdict:

```json
{"tier_pairs":[{"frontier":"mock-expensive","cheap":"mock-ok","active":true}]}
```

`active` is the honest part: a pair only applies while **both** models can be routed to
right now. Disable one of them and the pair reports `active: false` and stops applying:
it is shown, not silently dropped.

Save a pair and the next request follows it. **Routing → Recent changes** records the
change, who made it and what it was before.

## What a pair does to a request

Nothing, unless the automatic router is on
([the router and its threshold](04-auto-router-and-threshold.md)). With the router on
and a request that scores simple, the trace records the move:

```json
"router_event": {"reason": "heuristic_router", "from_model": "mock-expensive",
                 "to_model": "mock-ok", "pair_source": "tenant"}
```

`pair_source` says where the rule came from: `tenant` for a pair you added, `global`
for the built-in table. With the router off, the same request stays where it was asked
to go and `router_event` is absent.

## Choosing the cheap model

Three questions, in order:

1. **Does it answer as well?** Run your own evaluations before switching: pin a gold
   set from real traffic, run it on both models, and read the score
   ([datasets, gold sets and evals](10-datasets-gold-sets-evals.md)). A pair is a
   hypothesis; the eval is the test.
2. **Is it actually cheaper for your traffic?** A cheap model with a long system
   prompt can cost more than an expensive model with a short one. The Spend report
   prices per million tokens and per prompt
   ([reading the spend report](12-sessions-and-spend.md)).
3. **Does it accept what you send?** Structured output, tool calls and images are not
   universal. A `deepseek` provider cannot honour a `json_schema` contract; the gateway
   sends `json_object` instead and keeps the schema on our side for the checker
   ([turning on the output checker](03-output-checker.md)).

## Where the answers come from

A model id is served by the providers that own it. With one provider that is simple;
with several, the gateway spreads requests across the healthy ones and records why:

```json
"provider_selection": "weight"
```

`only` means one provider could serve it, `weight` means it was chosen by the adaptive
policy's health weights, `round_robin` means the requests were spread evenly. A
provider that fails its reachability test is skipped rather than tried.

## When to stop and ask

- A pair reports `active: true` and a request that should have been downgraded was not,
  with `router_event` absent.
- Two providers serve the same model and every request goes to the slower one.
- The cheap model's answers are visibly worse and the evaluation says they are fine:
  that is a gold set problem, not a pairing problem.

_Last verified 2026-11-06 against the live host (master 626609e)._
