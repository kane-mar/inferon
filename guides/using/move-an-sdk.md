# Move an existing SDK to Inferon

You change two values: the base URL and the API key. Model names, messages, tools
and streaming stay exactly as they are. This page shows the two values in Python,
Node and curl, and the four things that go wrong on the first try.

## What changes

| Setting | Before | After |
|---|---|---|
| Base URL | `https://api.deepseek.com/v1` (your provider) | `https://inferon.agilefederation.com/v1` |
| API key | your provider key | `sk-inferon-…` from **Settings → API keys** |

Your provider key does not go into your app. You paste it once into **Providers**
and the gateway uses it on your behalf. See
[Your first request](first-request.md) if you have not done that yet.

## Python, `openai` 3.x

```python
import os
from openai import OpenAI

client = OpenAI(
    api_key=os.environ["INFERON_API_KEY"],
    base_url="https://inferon.agilefederation.com/v1",  # this line is new
)

response = client.chat.completions.create(
    model="deepseek-flash",
    messages=[{"role": "user", "content": "Reply with the single word: ok"}],
)
print(response.choices[0].message.content, response.usage.total_tokens)
```

Output on the live host:

```text
ok 49
```

## Node, `openai` npm package

```js
import OpenAI from 'openai';

const client = new OpenAI({
  apiKey: process.env.INFERON_API_KEY,
  baseURL: 'https://inferon.agilefederation.com/v1', // this line is new
});

const response = await client.chat.completions.create({
  model: 'deepseek-flash',
  messages: [{ role: 'user', content: 'Reply with the single word: ok' }],
});
console.log(response.choices[0].message.content, response.usage.total_tokens);
```

Output on the live host, Node 24:

```text
ok 51
```

## curl

The same request without an SDK is in
[Your first request](first-request.md#5-send-the-request).

## The model name

Keep the model id your provider uses, for example `deepseek-flash`. Enabled models
are listed by `GET /v1/models` and in the provider's **Models** panel. If the
provider publishes no model list, the panel accepts ids you type.

An id the provider does not know is refused by the provider itself, and its message
names the ids it supports:

```json
{"error":{"message":"The supported API model names are deepseek-flash, deepseek-v4-pro, but you passed no-such-model-123. (request_id: …)","type":"invalid_request_error"}}
```

## What keeps working

- **Streaming.** Add `"stream": true` and you get the same server-sent events as
  before. The gateway passes each chunk through as it arrives.
- **Tools, system prompts, JSON mode.** Unchanged. The response format is the one
  your SDK expects.
- **Cost and latency.** The gateway records the request before it forwards it, so
  your own timing still includes one short hop.
- **Mixed shapes.** An Anthropic SDK can reach a DeepSeek provider and the reverse.
  The gateway translates the request and the response. A field that cannot be
  translated (an image for a text-only model, for example) returns a `400` that names
  the field instead of dropping it silently.

These calls need a provider of the matching type. An Anthropic SDK sends to
`/v1/messages`, so it needs a provider of type `Anthropic`. Point it at a workspace
with no Anthropic provider and the answer is:

```json
{"error":{"message":"no Anthropic provider configured — add one in Dashboard → Providers","type":"no_provider"}}
```

## Four things that go wrong

1. **The base URL ends in `/chat/completions`.** The SDK adds that path itself. The
   base URL ends at `/v1`.
2. **The provider key is in the SDK.** The SDK needs the Inferon key. The provider
   key lives only in **Providers**.
3. **The model is not enabled.** The error names the model; enable it on the
   provider, or use an id from `GET /v1/models`.
4. **The key is in a browser bundle or a mobile app.** Anyone who can read the bundle
   can spend your budget. Call the gateway from your server.

## Confirm it worked

Every answer carries a `x-inferon-trace-id` header. That id appears in the spend and
quality reports, so you can match a response your user complained about to the exact
request.

## If it did not work

Start with the table at the end of
[Your first request](first-request.md#if-it-did-not-work). A `401` is the key; a
`400` naming `no_provider` is the provider list.

_Last verified 2026-11-05 against the live host (master f2165a3)._
