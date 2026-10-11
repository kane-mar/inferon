## ITEM-051: Edge connector — reach customer-hosted (on-prem) LLMs

**As a** tenant running a model on my own network
**I want** the hosted proxy to reach it without exposing it or running a VPN
**So that** I can use my local LLM (LM Studio / Ollama / vLLM) with Inferon's cost
control and telemetry

Inferon is a hosted OpenAI-compatible proxy; a provider is a `base_url` the cloud
dials. A model on a customer LAN is unreachable — the platform's own deployment hit
this with a `Local LLM` provider at `http://192.168.1.224:1234/v1` (the cloud
cannot route to the LAN, so discovery timed out and every request hung). A
per-customer mesh VPN (Tailscale/WireGuard) does not scale operationally.

**Acceptance Criteria:**
1. A **"bring your own endpoint"** guide covers exposing an on-prem model over TLS
   (Caddy/nginx, Cloudflare Tunnel, ngrok) and using it as a provider; the
   Providers page surfaces reachability (the existing Test-connection probe).
2. A tenant can mint a **connector token** and run an `inferon edge` agent that
   opens an **outbound** connection (WSS over 443) to the control plane — no
   inbound firewall/NAT change.
3. The proxy forwards chat completions, including **streaming SSE**, over the
   tunnel to the customer's OpenAI-compatible endpoint; the **provider key never
   leaves the customer's network**.
4. Model discovery (`GET <local-base>/models`) is fetched by the edge and
   reflected in `provider_models`, the dashboard, and `GET /v1/models`.
5. Self-service and revocable: many tenants run independently on one relay;
   revoking the connector token drops its tunnel.
6. Failure is explicit: tunnel down → provider "unreachable" (fast-fail + alert);
   reconnect with backoff; per-request timeouts; backpressure on the stream.

**Technical Notes:**
- `provider.transport = direct | edge`; for `edge`, `base_url` is on-prem-only and
  the cloud never dials it.
- Protocol: WSS (or gRPC bidirectional) over 443; correlation ids; SSE chunk
  framing; reuse the provider-secret encryption seam if the token is stored.
- Relay: async Rust; many concurrent connections per node; per-tenant authz.
- Pairing: per-tenant connector token (mTLS later).
- Alternative for enterprise: **hybrid data plane** — run the full proxy on-prem
  and send telemetry to the cloud (same binary).
- Economics: tunneling inference through the cloud costs egress + latency; the
  public-endpoint route stays cheapest, hybrid best at scale.
- **First slice (low cost):** AC1 (the guide + connectivity check) — ships before
  the connector.

**Source:** 2026-10-11 design conversation — see
[raw/llm-inference/2026-10-11-local-llm-hosting-conversation.md](../../raw/llm-inference/2026-10-11-local-llm-hosting-conversation.md)
and [wiki/llm-inference/local-llm-hosting-and-edge-connector.md](../../wiki/llm-inference/local-llm-hosting-and-edge-connector.md).
