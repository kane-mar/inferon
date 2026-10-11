# Inferon Design Conversation: Hosting Customer Local LLMs (2026-10-11)

> Source: Recorded design discussion, Kane Mar (PO) with pi, 2026-10-11
> Collected: 2026-10-11
> Published: 2026-10-11

## Context (what led here)

- Inferon is a **hosted** OpenAI-compatible proxy. Tenants bring their own provider
  (BYO): a provider is a row with `kind`, `base_url`, an encrypted key, and a
  default flag; the proxy forwards a request to `<base_url>/chat/completions`.
- As of 2026-10-10/11 the model list is **discovered from the host**
  (`GET <base_url>/models`) and cached in `provider_models`; there is no static
  catalog. `GET /v1/models` returns the tenant's advertised models.
- The platform's shared upstream key is **not a pass-through**: self-serve
  tenants are keyless and must add a provider (migration 021).
- The PO registered a **`Local LLM`** provider at `http://192.168.1.224:1234/v1`
  (LM Studio on their LAN). The Vultr cloud host cannot route to that address, so
  model discovery timed out and every chat request hung. A 5s connect timeout was
  added so unreachable providers fail fast (`502 upstream_error`).
- This exposed the general product question: **how does a hosted control plane
  reach a model that lives on a customer's private network?**

## The question

> "If this is used as a hosted site, and the customer wants to host a local LLM
> … what's the best solution of enabling that? It's not possible to run Tailscale
> for many different users or clients. What's the best solution?"

## Key clarification: where does the caller live?

- **Customer app → Inferon's cloud URL** → the cloud must reach the model → a
  tunnel/relay is required.
- **Customer app runs on-prem too** → no tunnel: run the proxy locally and send
  only telemetry to the cloud (hybrid). Cheapest, lowest latency, no cloud egress
  for inference.

The "hosted site" case is the first one.

## Options considered

| Option | How it works | Pros | Cons |
|---|---|---|---|
| **A. Public endpoint** | Customer exposes the model behind TLS (Caddy/nginx, Cloudflare Tunnel, ngrok) and pastes the URL as the provider `base_url` | Zero backend work (already supported) | Exposes the model; per-customer netops; auth/DDoS |
| **B. Edge connector** | Customer runs a small `inferon edge` agent that dials **out** over 443 and reverse-tunnels requests to the local model | No inbound/NAT change; self-service; **provider key stays on-prem**; works through any HTTPS proxy | Must build the relay + agent; inference traverses the cloud (bandwidth/latency); streaming framing |
| **C. Hybrid data plane** | Customer runs the full proxy on-prem; the cloud is the **control plane** (auth, dashboard, config, telemetry aggregation) | No tunnel; LLM traffic never leaves the LAN (privacy, latency, no egress cost); reuses the existing binary/compose | Customer runs infrastructure; split-plane design |
| **D. Managed VPN** (WireGuard/Tailscale per customer) | The cloud node peers with each customer | Conceptually simple | Per-peer config, NAT/firewall fragility, IP management — the operational load to avoid |

Tailscale/ngrok/Cloudflare Tunnel all share the same "agent dials out" shape; a
mesh VPN is the wrong tool when the goal is many independent customers.

## Recommendation (layered)

1. **Ship A now** as a documented recipe (Cloudflare Tunnel / ngrok / Caddy in
   front of Ollama/vLLM/LM Studio) — no backend work; unblocks customers who can
   expose a hostname. Add a **connectivity check** on the Providers page (the
   `Test connection` action already probes `GET <base>/models`).
2. **Build B** as the product feature for "local LLM + hosted Inferon" — a
   first-party outbound **edge connector**. This is the scalable answer.
3. **Offer C** for enterprise (privacy, volume, latency) — the same Rust binary
   in edge mode, cloud keeps the control plane.

## Edge connector (B) — design sketch

- **Transport**: WSS (or gRPC bidirectional) over **443** — WSS traverses
  corporate proxies most reliably. Multiplex concurrent requests with a
  correlation id; frame SSE chunks for streaming responses.
- **Pairing**: mint a per-tenant **connector token** in the dashboard; the agent
  presents it to bind the tunnel to that tenant; revocable like an API key.
- **Security**: TLS + token (mTLS later). Because the upstream key never leaves
  the customer's network, the edge holds the provider credential and talks to the
  model; the cloud only relays bytes. A further step is for the edge to compute
  usage/cost and send **metadata only** (privacy-preserving) while the cloud does
  guard/cost on the payload path if retained.
- **Model discovery**: the edge fetches `GET <local-base>/models` and reports the
  list up, feeding the existing `provider_models` cache and `GET /v1/models`.
- **Data model**: `provider.transport = direct | edge`. For `edge`, `base_url` is
  the on-prem URL only the connector uses; the cloud never dials it.
- **Failure handling**: tunnel down → provider "unreachable" (reuse the 5s
  fast-fail and alerting); reconnect with backoff; per-request timeouts;
  backpressure.

## Economic caveat

Tunneling inference through the cloud means every prompt/completion byte crosses
it — egress cost plus latency. For large models and long generations this can
dominate. Position the connector as a hosted convenience; offer the hybrid data
plane (C) for scale. The public-endpoint route (A) remains the cheapest option
for customers who can expose an endpoint.

## Outcome

- Capture the design as a backlog item: **ITEM-051** — Edge connector for
  customer-hosted (on-prem) LLMs.
- First slice: the A recipe + a connectivity check (low cost). Then the WSS
  connector + relay.
- Open questions: connector protocol (WSS vs gRPC), whether the cloud sees the
  payload (guard/cost) or the edge reports metadata only, and the relay's
  bandwidth accounting.
