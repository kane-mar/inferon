# Your first request

This page takes you from signing in to a request answered by a model, with the
request visible in your dashboard. It takes about five minutes. You need a GitHub
account and a key from a model provider.

## 1. Sign in

Open `https://inferon.agilefederation.com/sign-in` and sign in with GitHub.

Your first sign-in creates a workspace (tenant) that you own and opens **Settings →
Connect your SDK**. Every later sign-in goes straight to your dashboard.

## 2. Create an API key

In the **Connect your SDK** card, press **Create my key**.

The key starts with `sk-inferon-` and is shown once. We keep a hash, never the key,
so copy it now. Put it in your shell:

```bash
export INFERON_API_KEY='sk-inferon-…'
```

You can create more keys, and revoke any of them, on **Settings → API keys**.

## 3. Add a provider

A new workspace has no provider, so the gateway has nowhere to send a request. In
the same card, press **Providers →** (or open **Providers** in the sidebar). The page
says **No provider yet — add one so requests have somewhere to go.** Press **Add
provider**.

| Field | What to enter |
|---|---|
| **Type** | `DeepSeek` (or `OpenAI`, `OpenAI-compatible`, `Anthropic`, `local`) |
| **Name** | anything you recognise, e.g. `DeepSeek` |
| **Base URL** | filled in for you: `https://api.deepseek.com/v1` for DeepSeek |
| **API key** | the key from your provider account |

Press **Add provider**, then **Test** on the new row. **Connected — N models** means
Inferon reached the provider and read its model list. **Cannot reach it** means the
URL, the key or your network is wrong.

The gateway runs on your provider account, not on a shared one. A key from your
provider is what makes the first request possible.

## 4. Choose a model

Press **Models** on the provider row. The models it reports are listed there; enable
the ones you want and press **Make default** on the one you reach for most. Enabled
models are what `GET /v1/models` returns and what the pickers offer. A provider that
accepts an id you have not enabled still answers it, and an id it does not know
comes back as its own `400`, listing the ids it supports.

Ask the API for your list:

```bash
curl -s https://inferon.agilefederation.com/v1/models \
  -H "Authorization: Bearer $INFERON_API_KEY"
```

```json
{"data":[{"created":0,"id":"deepseek-flash","object":"model","owned_by":"inferon"},
         {"created":0,"id":"deepseek-v4-pro","object":"model","owned_by":"inferon"}],
 "object":"list"}
```

## 5. Send the request

```bash
curl -s -D - https://inferon.agilefederation.com/v1/chat/completions \
  -H "Authorization: Bearer $INFERON_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"deepseek-flash","messages":[{"role":"user","content":"Say hi in three words."}]}'
```

A good response is HTTP 200, a `x-inferon-trace-id` header, and a body like this:

```json
{"id":"a2d5e108-…","object":"chat.completion","model":"deepseek-flash",
 "choices":[{"index":0,"message":{"role":"assistant","content":"Hi there friend"},
             "finish_reason":"stop"}],
 "usage":{"prompt_tokens":35,"completion_tokens":18,"total_tokens":53}}
```

The text you want is `choices[0].message.content`. The `x-inferon-trace-id` value
names this request in every report.

## 6. Confirm it was recorded

Go back to **Settings**. The Connect card now says **First request received**.

Open **Dashboard**. Within a second or two, **Requests**, **Total cost** and **Spend
by model** count the request. **Settings → API keys** shows it under **Last used**.

The request is saved with its cost, tokens, latency and outcome. If you turned on
prompt capture, the prompt and the answer are saved too.

## If it did not work

| What you see | What it means | Fix |
|---|---|---|
| `401` `{"error":{"message":"missing or invalid Authorization header","type":"invalid_api_key"}}` | The request carried no key, or a key we do not know. | Send `Authorization: Bearer $INFERON_API_KEY`. If the key was revoked, create a new one. |
| `400` `{"error":{"message":"no upstream provider configured — add one in Dashboard → Providers","type":"no_provider"}}` | The workspace has no provider, so there is nothing to forward to. | Do step 3 above. |
| `400` `{"error":{"message":"no Anthropic provider configured — add one in Dashboard → Providers","type":"no_provider"}}` | You called `/v1/messages`, which needs a provider of type `Anthropic`. | Add an `Anthropic` provider, or call `/v1/chat/completions`. |

## Next steps

- [Move an existing SDK to Inferon](move-an-sdk.md) — Python, Node or curl, two lines.
- [Inferon on your own network](../inferon-self-hosted.md) — run the same stack yourself.

_Last verified 2026-11-05 against the live host (master f2165a3)._
