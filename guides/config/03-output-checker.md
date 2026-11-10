# Turning on the output checker

The checker (the product calls it the **output checker**; the logs call it the *guard*)
does one thing: when a request carries a JSON-schema contract, it verifies that the
answer actually satisfies it before the client sees it, and replays the request on a
stronger model when it does not.

## What it needs

Three things, and it does nothing without all three:

1. **A contract.** The request must carry
   `response_format.json_schema.schema` : the OpenAI structured-output shape. A request
   without it is not checked, because there is nothing to check.
2. **A cheap model** serving it. "Cheap" means: one of the built-in families
   (`gpt-4o-mini`, `deepseek-chat`, `deepseek-flash`, `deepseek-v4-flash`), the cheap
   side of a pair **you** defined
   ([models and cheaper pairs](02-models-and-tier-pairs.md)), or any model you host
   (`kind: local`). The point of the checker is to catch the cheap model being wrong, so
   a frontier model is never checked.
3. **The switch on.** It ships on. **Routing → Controls → Output checker** turns it off
   per workspace.

## What the client sees

If the answer satisfies the schema, it is delivered unchanged and the trace records:

```json
"guard_event": {"outcome": "flushed", "guarded_model": "mock-ok"}
```

If it does not, the answer is **thrown away**, the prompt is replayed on a stronger
model, and the client receives that answer instead : one request, one response, no retry
logic in your code:

```json
"guard_event": {"reason": "schema_violation", "outcome": "fallback",
                "guarded_model": "mock-lie"}
```

`guarded_model` is the model that lied; the trace's `request_model` is the model that
ultimately served (and what the cost is based on). With a fallback step marked for the
checker, the replay goes to that step and the trace also carries
`"reason": "guard"` in its `routing_event`.

When the replay is wrong as well, the trace says so rather than pretending:

```json
"guard_event": {"reason": "schema_violation", "outcome": "fallback_invalid",
                "guarded_model": "mock-lie"}
```

## Streaming and non-streaming are both checked

A streamed answer is buffered, assembled and validated before the first byte reaches the
client; a plain (non-streamed) answer is validated the same way. If your client streams,
it waits for the whole answer before the first chunk arrives , that is the cost of the
guarantee, and it is the reason the checker is only applied to cheap models.

## DeepSeek, and any provider without `json_schema`

DeepSeek's API accepts `json_object`, not `json_schema`. Rather than refuse the request,
the gateway sends the provider `{"type": "json_object"}`, keeps your schema on our side,
and validates the answer against it. The upstream body proves it : this is what the
provider actually received for a `json_schema` request:

```json
{"model":"deepseek-flash","messages":[…],"response_format":{"type":"json_object"}}
```

The trace records `"wire_format": "json_object"` so the downgrade is visible rather than
implied. Your client still gets the answer it asked for, and the contract is still
enforced.

## Turning it off

**Routing → Controls → Output checker** off. Requests then pass through unchanged, and a
schema-violating answer reaches your client , which is a choice you are allowed to make,
for example while debugging a provider. Nothing else changes: the contract still travels
to the provider, and the trace records no `guard_event`.

Prefer narrowing it to turning it off? Leave the checker on and remove the pair or
disable the cheap model: with no cheap model serving the request, no answer is checked.

## How to confirm it worked

- **Routing → Recent changes** shows the switch changing, with who and when.
- **Quality → Checker fallback** counts the replays; a working checker on a healthy
  provider is close to zero.
- A trace with a `guard_event` names the outcome, the reason and the model that lied.

## When to stop and ask

- The checker is on and a schema-violating answer reached your client, with no
  `guard_event` on the trace.
- `fallback_invalid` keeps appearing: your replay target is failing too, which is a
  provider problem, not a checker problem.
- Structured answers are correct but arrive with the first byte much later than before:
  that is the buffering, and it is expected for a cheap model.

_Last verified 2026-11-06 against the live host (master acdaffc)._
