## ITEM-051: Customer-hosted endpoint (direct / BYO endpoint)

**As a** tenant running a model on my own network
**I want** to expose it behind TLS and point Inferon at it
**So that** the hosted proxy serves my local LLM with no cloud relay in the path

This is option **A** of the customer-hosted connectivity decision (2026-10-11):
the customer publishes their OpenAI-compatible endpoint and registers it as a
provider `base_url`; the cloud proxy dials it directly. No reverse tunnel — the
only added hop is the proxy itself. (The edge connector / option B is deferred;
the hybrid data plane / option C is ITEM-052.)

**Acceptance Criteria:**
1. A **"bring your own endpoint" recipe** documents exposing an on-prem
   OpenAI-compatible server (Ollama, LM Studio, vLLM) behind TLS — Caddy/nginx,
   Cloudflare Tunnel, or ngrok — and registering it as a provider.
2. The Providers **add** form can test the connection **before** the provider is
   created (a probe of an unsaved config, `POST /providers/test`); the saved
   per-provider Test-connection re-checks it. Reachability is persisted per
   provider (reachable / unreachable, last checked) so a customer can verify an
   exposed endpoint from the cloud.
3. The recipe covers scoping (a dedicated provider key, extra headers) and the
   security posture (TLS required for public endpoints; prefer allow-listing).
4. Model discovery (`GET <base>/models`) works against the exposed endpoint and
   feeds `provider_models` / `GET /v1/models` (already implemented).

**Technical Notes:**
- No new transport: `provider.base_url` already supports any HTTPS endpoint;
  custom headers/org are supported (ITEM-043).
- Persist the probe result: `providers.last_check_at`, `providers.last_check_ok`;
  `POST …/providers/:id/test` records it; `provider_json` exposes it.
- Latency: client → cloud → customer endpoint → model. Fine for most; use the
  hybrid data plane (ITEM-052) when the model is latency-critical and the caller
  is on-prem.

**Source:** 2026-10-11 customer-hosted connectivity decision — see
[raw/llm-inference/2026-10-11-customer-hosted-connectivity-decision.md](../../raw/llm-inference/2026-10-11-customer-hosted-connectivity-decision.md)
and [wiki/llm-inference/customer-hosted-llms.md](../../wiki/llm-inference/customer-hosted-llms.md).
