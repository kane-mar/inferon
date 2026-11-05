# 5xx and the fallback chain

A `5xx` from Inferon is either the provider's own error, passed through, or a `502` of
ours that means nothing was sent at all. The fallback chain changes what you receive.

## Symptom

Your client gets a `5xx`, and the body is one of three things:

```json
{"error":{"message":"<the provider's own message>","type":"server_error"}}
```

```json
{"error":{"message":"upstream request failed","type":"upstream_error"}}
```

```json
{"error":{"message":"<the next step's error>","type":"..."}}
```

## What it means

- **The provider's own body, with its status.** The call reached the provider and it
  failed. `502`, `503` and `500` from the provider arrive unchanged, with the
  provider's `type` and message.
- **`upstream_error` with `502`.** Inferon could not connect at all: refused, wrong
  host, TLS failure, or a timeout. No provider saw the request. Nothing was billed.
- **A different error than the model you asked for.** A fallback step ran and failed
  too. You are seeing the last attempt's error, because an exhausted chain serves the
  upstream error rather than inventing one.

The trace records which of these happened. An upstream `500` is class `upstream_5xx`,
a connection failure is class `transport`, and a chain that changed model records the
move:

```json
"routing_event": {"step": 1, "reason": "upstream_5xx", "to_model": "deepseek-flash", "from_model": "deepseek-v4-pro"}
```

## Fix

1. **Read the body first.** A provider's `type` and message means the provider failed.
   `upstream_error` means we could not reach it.
2. **Check the class** on **Quality → Errors by cause**: `upstream_5xx` or `transport`.
   That separates "their bad minute" from "our bad config".
3. **For a provider failure**, add a fallback step with `5xx` in **Handles**
   (**Routing → Fallback chain**). The next model runs instead of returning the error.
4. **For `transport`**, check the provider's **Base URL** and press **Test**. For a
   `local` provider, the gateway must be able to reach that address; a `127.0.0.1` on
   your laptop is not your laptop when the gateway is remote.
5. **Retry once the provider recovers.** `Retry-After` is honoured only for `429`; a
   `5xx` is retried by the chain, not by the gateway.

## How to confirm it worked

- The request returns `200` and the response carries `x-inferon-trace-id`.
- **Providers** shows **Connected** on the row you fixed, after **Test**.
- **Quality → Errors by cause** stops growing `transport` (or `upstream_5xx`) for that
  model.
- With a chain, the trace names the step that served the request, so a `200` from the
  fallback is visible rather than inferred.

## When to stop and ask

- `502` `upstream_error` with a base URL and key that work from your own machine. The
  path from the gateway to the provider is the problem.
- Every step in the chain fails within a minute, and the provider's status page is
  green.
- The chain has a step with the `5xx` trigger and the client still receives the first
  step's failure.
- You changed the chain and want to know what actually ran; **Routing → Recent changes**
  and the trace agree on who changed it and when.

_Last verified 2026-11-05 against the live host (master 09f47d8)._
