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

## C — Hybrid data plane

Run the same Rust binary on the customer's network as an **edge**:

```
client ──> edge proxy (on-prem) ──> local model
                 │
                 └── outbound, batched: telemetry + config ──> cloud control plane
```

- **Data plane (edge):** serves the OpenAI-compatible surface locally, resolves
  the local provider directly (`base_url` never leaves the network), authenticates
  client keys against synced key hashes, and applies model selection, routing,
  budget, and guard locally.
- **Control plane (cloud):** auth/sessions, the dashboard, provider/model/
  routing/budget config, alerts, and **telemetry ingestion** to power the
  dashboard and billing.
- **Uplink:** only small, batched control + telemetry leaves the network — never
  inference. This is the latency win.
- **Enrollment:** a per-tenant edge token; the node syncs its config and pushes
  traces under a node credential (revocable).

### First slice (backbone)

The cloud-side **telemetry ingest** endpoint: an edge pushes batches of trace
records authenticated as its tenant; the cloud writes them through the existing
sink so the dashboard/billing/alerts light up. Everything else (edge mode,
config sync) builds on it.

## Economics & performance

| Path | Inference hops | Cloud egress | Latency |
|---|---|---|---|
| A direct endpoint | client → cloud → customer → model | completion body | +1 cloud hop |
| C hybrid | client → local proxy → model | telemetry only | minimal |
| B edge connector *(deferred)* | client → cloud → tunnel → model | full payload | highest |

## Status

- **ITEM-051** — customer-hosted endpoint (A): recipe + reachability.
- **ITEM-052** — hybrid data plane (C): the control/data split, edge mode,
  config sync, telemetry ingest.
- B (edge connector) is deferred in the backlog record, not built.

## See Also

- [Inferon Project Direction](inferon-project-direction.md)
- [Inferon Product Roadmap](inferon-product-roadmap.md)
- [AIO-Proxy: Autonomous Inference Optimizer](aio-proxy-overview.md)
