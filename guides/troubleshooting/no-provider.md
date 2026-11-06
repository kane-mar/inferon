# no_provider: the workspace has nowhere to send the request

This is the most common first failure. The gateway refuses before it contacts anyone.
The whole path from signing in to an answered request is in
[Your first request](../using/first-request.md).

## Symptom

```
$ curl -s -w ' [%{http_code}]\n' https://inferon.agilefederation.com/v1/chat/completions \
    -H "Authorization: Bearer $INFERON_API_KEY" -H 'Content-Type: application/json' \
    -d '{"model":"deepseek-flash","messages":[{"role":"user","content":"hi"}]}'
{"error":{"message":"no upstream provider configured — add one in Dashboard → Providers","type":"no_provider"}} [400]
```

The Anthropic surface answers with its own wording:

```json
{"error":{"message":"no Anthropic provider configured — add one in Dashboard → Providers","type":"no_provider"}}
```

`GET /v1/models` still returns `200`, with an empty list:

```json
{"data":[],"object":"list"}
```

## What it means

The workspace has no provider that can take this request. There are three shapes of it:

- **No provider at all.** A self-serve workspace starts this way. Its traffic cannot
  use a shared platform key; the platform key only exists on workspaces an operator
  seeded by hand.
- **No provider for the surface.** You called `/v1/messages` (an Anthropic client) and
  every provider you added is OpenAI-shaped. Anthropic calls need a provider of type
  `Anthropic`.
- **No provider for the model.** A provider exists, but the model you asked for is not
  served by any of them.

**The refusal is recorded.** The answer carries `x-inferon-trace-id`, and that id is
the row **Quality → Errors by cause** counts under the class `inferon_4xx`. Nothing was
sent upstream, so there is no spend and no provider traffic — but the attempt is
visible, with its model and its time.

## Fix

1. Open **Providers**. The empty state says **No provider yet — add one so requests
   have somewhere to go.** Press **Add provider**.
2. Choose the **Type** that matches your account: `DeepSeek`, `OpenAI`,
   `OpenAI-compatible`, `Anthropic`, or `local` for a model on your own network.
3. The **Base URL** is filled in for the two named kinds (`https://api.deepseek.com/v1`,
   `https://api.openai.com/v1`). For anything else, paste the endpoint that ends at
   `/v1`. The key is the provider's own key, and a `local` provider needs no key.
4. Press **Add provider**, then **Test** on the new row. You want
   **Connected — N models**.
5. Press **Models** on the provider row and enable the model you ask for. Press
   **Make default** on the one you reach for most.
6. For an Anthropic client, add a provider of type `Anthropic` and send the request to
   `/v1/messages` as your SDK already does.

## How to confirm it worked

```bash
curl -s https://inferon.agilefederation.com/v1/models -H "Authorization: Bearer $INFERON_API_KEY"
```

The list is no longer empty, and a completion returns `200` with a
`x-inferon-trace-id` header:

```json
{"data":[{"created":0,"id":"deepseek-flash","object":"model","owned_by":"inferon"}],"object":"list"}
```

Then **Settings → Connect your SDK** flips to **First request received**, and
**Dashboard** counts the request within a second or two. If the request is still
refused after this, the answer is a different one — read the body again.

## When to stop and ask

- **Test** says **Cannot reach it** with a base URL and key you know are right. That is
  a network or TLS problem between the gateway and the provider, not a workspace
  setting.
- A provider is connected, a model is enabled, and the request still answers
  `no_provider` for the OpenAI surface.
- You need an Anthropic provider and do not have an Anthropic account.
- Something other than you seeded this workspace and you cannot tell which key it uses.

_Last verified 2026-11-05 against the live host (master 3da7bd3)._
