# Local LLM Hosting & the Edge Connector

> Raw: [2026-10-11-local-llm-hosting-conversation.md](../../raw/llm-inference/2026-10-11-local-llm-hosting-conversation.md)
> Updated: 2026-10-11

## Overview

Inferon is a hosted OpenAI-compatible proxy with bring-your-own providers. A
tenant's provider is a `base_url` the **cloud** proxy dials. When a customer's
model runs on their own LAN (LM Studio, Ollama, vLLM), the cloud cannot route to
it. This article records the options and the chosen direction for reaching
customer-hosted models.

## Problem

Cloud → private network. The caller's location decides the solution:

- **Caller in the cloud** → the proxy must reach the model → a tunnel/relay.
- **Caller on-prem** → run the proxy on-prem; the cloud is only the control
  plane.

A per-customer mesh VPN (Tailscale/WireGuard) is the wrong tool: it demands
per-peer networking, breaks behind NAT/firewalls, and does not scale
operationally. Tailscale/ngrok/Cloudflare Tunnel are all the same pattern — an
agent that dials **out** — just operated by a third party.

## Options

| Option | Mechanism | Verdict |
|---|---|---|
| **A. Public endpoint** | Customer exposes the model behind TLS; paste the URL as `base_url` | Zero build; works today; expose/security cost |
| **B. Edge connector** | Customer runs `inferon edge`; outbound WSS reverse tunnel to the cloud | Recommended product feature |
| **C. Hybrid data plane** | Run the proxy on-prem; cloud = control plane | Best for privacy/scale |
| **D. Managed VPN** | WireGuard/Tailscale per customer | Avoid |

## Recommended direction

Layered:

1. **A now** — a documented recipe (Caddy/Cloudflare Tunnel/ngrok) + a
   connectivity check on the Providers page (the `Test connection` probe already
   hits `GET <base>/models`).
2. **B as the feature** — a first-party, self-service **edge connector**.
3. **C for enterprise** — the same binary in edge mode; cloud keeps auth,
   dashboard, config, and telemetry aggregation.

## Edge connector shape

- Outbound **WSS over 443**, request multiplexing by correlation id, SSE chunk
  framing for streaming.
- Per-tenant **connector token** for pairing (revocable); TLS now, mTLS later.
- The **provider key stays on-prem**; the edge talks to the model, the cloud
  relays. Optionally the edge reports usage/cost metadata only.
- Model discovery flows up into the existing `provider_models` cache and
  `GET /v1/models`.
- `provider.transport = direct | edge`; `base_url` is on-prem-only for `edge`.
- Tunnel loss degrades to "provider unreachable" (5s fast-fail + alert).

## Economics

A reverse tunnel puts **all inference bytes through the cloud** (egress + latency
cost). For large models/long generations the hybrid data plane or a public
endpoint is materially cheaper. The connector is a hosted convenience, not the
cheapest path.

## Status

Captured as backlog **ITEM-051** (edge connector), with the A recipe + connectivity
check as its first slice. See [Inferon Product Roadmap](inferon-product-roadmap.md)
for where connectivity sits relative to the autonomy roadmap.

## See Also

- [Inferon Project Direction](inferon-project-direction.md)
- [Inferon Product Roadmap](inferon-product-roadmap.md)
- [AIO-Proxy: Autonomous Inference Optimizer](aio-proxy-overview.md)
