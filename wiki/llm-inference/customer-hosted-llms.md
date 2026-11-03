# Customer-Hosted LLMs

> Raw: [2026-10-11-local-llm-hosting-conversation.md](../../raw/llm-inference/2026-10-11-local-llm-hosting-conversation.md); [2026-10-11-customer-hosted-connectivity-decision.md](../../raw/llm-inference/2026-10-11-customer-hosted-connectivity-decision.md)
> Updated: 2026-10-11

## Overview

Inferon is a hosted OpenAI-compatible proxy. Its provider model points at a
`base_url` the **cloud** dials. When a customer's model runs on their own network
(LM Studio, Ollama, vLLM), the cloud cannot route to it directly. The chosen
direction is a **control-plane / data-plane split**, delivered in two shapes:

- **A — direct endpoint:** the customer exposes the model behind TLS; the cloud
  proxy dials it as a normal provider.
- **C — hybrid data plane:** the customer runs the **proxy on their network**
  against the local model; the cloud is the control plane only.

**B — an outbound edge connector that relays inference through the cloud — is
deferred.** It is convenient (no inbound/NAT change) but puts every token on a
cloud relay: added latency and egress. Performance is the deciding constraint.

## A — Direct endpoint

The customer publishes their OpenAI-compatible endpoint (Caddy/nginx + TLS,
Cloudflare Tunnel, ngrok, or an LB) and registers it as a provider `base_url`
with their key. The proxy forwards `chat/completions` and discovers models via
`GET <base>/models` (the existing `provider_models` cache and `/v1/models`).

### Recipes

**Ollama (LAN host `192.168.1.224:11434`) behind Caddy on a small VM/edge box:**

```caddy
llm.customer.example {
    reverse_proxy 192.168.1.224:11434
}
```
Register `https://llm.customer.example/v1` as an `openai-compatible` provider.

**LM Studio (`:1234`) via Cloudflare Tunnel (no inbound ports):**

```bash
cloudflared tunnel --url http://localhost:1234
# -> https://<random>.trycloudflare.com  (use .../v1 as the provider base_url)
```

**ngrok:**

```bash
ngrok http 1234
# -> https://<id>.ngrok.app  (provider base_url: https://<id>.ngrok.app/v1)
```

Then: Dashboard → Providers → add (`openai-compatible`, the URL, a key) → **Test
connection** (before saving) → **Test connection** again on the saved row. The
probe (`GET <base>/models`) records reachability and caches the model list.

> The platform's own shared upstream connection is **Inferon-private**: it is not
> shown on the tenant Providers page — tenants always bring their own provider.

### Posture

- **TLS only** for public endpoints; use a dedicated provider key and, where
  possible, allow-list Inferon's egress IPs.
- The endpoint is reachable from the cloud; anything else on the customer's LAN
  stays private.
- Latency: `client → cloud → customer endpoint → model` — one proxy hop.

## Retired: the edge node (2026-11-01)

An earlier design ran a stripped proxy on the customer's network ("edge node")
that synced config from the cloud and shipped telemetry back. It is **retired**,
and the reason is structural rather than a matter of maturity:

- Prompts never leave the node, so the cloud could only ever keep records.
  Datasets, eval, prompt versioning, the heatmap and policy adaptation all live
  where the traces are — none of it could run on the node.
- The node's own controls were a **stale cloud snapshot**: spend came from the
  last sync (never incremented locally), and of the 17 config fields the cloud
  sent, the runtime parsed 9 and hardcoded the rest. A privacy node that cannot
  enforce a budget or a rate limit is not a controller.
- Everything it did, the full package does better — **inside the customer's own
  network** — and it is the same artifact we already run: `--profile
  self-hosted` (proxy + web + Postgres + Redis) gives them the dashboard,
  datasets, adaptation and audit locally.
- It was a second data plane with a different storage model, so every feature
  had to be built twice and the node version was always the lesser one.

## Profiles today — two

| Profile | Customer setup | Prompts transit cloud | Backlog |
|---|---|---|---|
| **Self-hosted** (full stack on their Docker) | Docker on their host | **No** | ITEM-083 delivered |
| **Cloud + reachable endpoint** | Publish the model behind TLS | Yes (one extra hop) | ITEM-051/077/081/082 delivered |

There is no profile in which the cloud routes to an on-prem model and the prompt
stays off the cloud: routing through the cloud *is* sending the prompt through
the cloud. So privacy means either the full stack on the customer's side, or the
model stays theirs while prompts transit (the reachable endpoint). The connector
(option B / ITEM-084) stays staged until a customer refuses both.

## Economics & performance

| Path | Inference hops | Cloud egress | Latency |
|---|---|---|---|
| A direct endpoint | client → cloud → customer → model | completion body | +1 cloud hop |
| C hybrid | client → local proxy → model | telemetry only | minimal |
| B edge connector *(deferred)* | client → cloud → tunnel → model | full payload | highest |

## Status

- **ITEM-051** — customer-hosted endpoint (A): recipe + reachability. Built.
- **ITEM-052** — hybrid data plane (C): the control/data split, edge mode,
  config sync, telemetry ingest. Built.
- **ITEM-082** — the five-minute attach flow for A (keyless `local`, badge,
  periodic reachability). Built 2026-10-29.
- **ITEM-083** — self-hosted distribution of the full stack. Built 2026-10-29;
  install guide in `guides/inferon-self-hosted.md`.
- B (edge connector) is **staged** as ITEM-084, not built, and the edge node is
  explicitly not a cloud fallback.

## See Also

- [Inferon Project Direction](inferon-project-direction.md)
- [Inferon Product Roadmap](inferon-product-roadmap.md)
- [AIO-Proxy: Autonomous Inference Optimizer](aio-proxy-overview.md)
