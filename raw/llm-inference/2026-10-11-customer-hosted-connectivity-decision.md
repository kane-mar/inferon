# Inferon Decision: Customer-Hosted LLM Connectivity — A + C (2026-10-11)

> Source: Recorded product decision by Kane Mar (PO) via pi session, 2026-10-11
> Collected: 2026-10-11
> Published: 2026-10-11

Decision: pursue **A (direct endpoint)** and **C (hybrid data plane)** for
reaching customer-hosted LLMs. **B (outbound edge connector that relays inference
through the cloud) is deferred/rejected** — it puts every prompt/completion byte
on a cloud relay, adding latency and egress cost, and this product needs to be
performant.

Rationale, per option:

- **A — direct endpoint (keep):** the customer exposes their model behind TLS
  (Caddy/nginx, Cloudflare Tunnel, ngrok, a load balancer) and registers the URL
  as the provider `base_url`; the cloud proxy dials it directly. No relay
  buffering; the only added hop is the proxy itself. Cheapest and simplest.
- **C — hybrid data plane (build):** run the **full proxy on the customer's
  network** (same Rust binary) against the local model, so inference is
  `client → local proxy → local model` — the latency-optimal path. The cloud is
  the **control plane** only: auth/sessions, provider/model/routing/budget
  config, and **telemetry ingestion** for the dashboard, alerts, and billing. The
  only outbound traffic from the edge is small, batched control + telemetry — not
  inference.
- **B — edge connector (deferred):** a reverse tunnel that proxies inference
  through the cloud. Convenient (no inbound/NAT changes) but directly at odds
  with the latency requirement. Reconsider only if a customer cannot expose an
  endpoint *and* cannot run the edge.

Implication for the architecture: define a clear **control-plane / data-plane
split**. The data plane currently lives in the cloud; with C it can also run at
the edge. That split is the concrete engineering direction.
