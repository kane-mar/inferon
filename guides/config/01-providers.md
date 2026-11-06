# Add your own provider

A provider is the account the gateway calls on your behalf. Until you add one, a
request has nowhere to go and is refused with
[no_provider](../troubleshooting/no-provider.md).

Open **Providers** in the sidebar and press **Add provider**.

## The five types

| Type | What it is | Base URL | Key |
|---|---|---|---|
| `deepseek` | DeepSeek's own API | `https://api.deepseek.com/v1` | required |
| `openai` | OpenAI's own API | `https://api.openai.com/v1` | required |
| `openai-compatible` | Anything that speaks the OpenAI chat API (vLLM, Ollama, LM Studio, a gateway) | yours, ending at `/v1` | required |
| `anthropic` | Anthropic's Messages API | `https://api.anthropic.com/v1` | required |
| `local` | A model on your own network | yours, ending at `/v1` | **not needed** |

That list is what the API returns, so the form can never offer a type the gateway
does not know:

```bash
curl -s https://inferon.agilefederation.com/v1/dashboard/api/providers \
  -H "Cookie: inferon_session=$SESSION"
```

```json
{"kinds":["deepseek","openai","openai-compatible","anthropic","local"],"providers":[],"role":"owner"}
```

The four keyed types refuse an empty or missing key with `400 invalid_key`
(`"a provider key is required"`). `local` needs no key at all — the field can be
omitted entirely, and a local provider is stored without one.

## The key

The provider's key is encrypted at rest and never shown again. The dashboard keeps a
masked hint — the last four characters:

```json
{"kind":"deepseek","name":"Example","base_url":"https://api.deepseek.com/v1","key_hint":"…-key","is_default":false}
```

If you need to change it, open **Edit** on the provider and use **API key (replace)**.
Leaving that field blank keeps the current key.

## Test it

Press **Test** on the provider's row. The gateway calls the provider's `/models`
endpoint and reports what came back:

```json
{"ok":true,"status":200,"models":4,"error":null}
```

A failure says what the provider said, so you can tell a wrong key from a wrong URL:

```json
{"ok":false,"status":401,"models":0,"error":"upstream returned 401 Unauthorized"}
```

The result is stored on the provider and shown as **Connected**, **Cannot reach it** or
**Not tested**. If it says **Cannot reach it**, the problem is between the gateway and
that URL — see
[a provider that tests as unreachable](../troubleshooting/provider-unreachable.md).

## Pick the models

Press **Models** on the provider row. The list is what the provider advertises; enable
the ones you want and press **Make default** on the one you reach for most. Models are
listed per provider, because the same id can be served by two of them:

```json
{"id":"…","name":"Guide mock","kind":"openai-compatible","is_default":true,
 "models":[{"id":"mock-ok","enabled":true,"is_default":true},
           {"id":"mock-expensive","enabled":true,"is_default":false}]}
```

Only enabled models answer. The first provider you add becomes the workspace default —
the one used for a model id no provider claims.

## A model on your own network

Choose type `local`. The key field disappears, because a model on your own hardware
needs no credential, and its traces are costed at zero.

Two things matter:

- **The gateway must be able to reach the URL.** If you run Inferon in the cloud, a
  `http://127.0.0.1:8080/v1` endpoint is the *container's* loopback, not your machine.
  Expose the model on the network, or run the self-hosted stack
  ([Inferon on your own network](../inferon-self-hosted.md)).
- **Use HTTPS if the URL is public.** Plain HTTP over the internet is a credential
  leak waiting to happen, and providers behind TLS terminate it for you.

## Remove or replace

**Remove** on the provider's row deletes it and its models. Requests that relied on it
start failing with `no_provider` on the next call, so add the replacement first.

Every provider change is recorded: **Routing → Recent changes** shows who changed what
and when, and the change is visible to the whole workspace.

## When to stop and ask

- **Test** fails with a URL and key you know work from your own machine.
- The provider's `/models` answers, but a completion returns its own `400` about the
  model name.
- You need a provider type that is not in the five above — the gateway has no adapter
  for that API shape, and that is a feature request, not a setting.

_Last verified 2026-11-06 against the live host (master 626609e)._
