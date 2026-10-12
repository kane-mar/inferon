## ITEM-065: Edge node mode & config sync (hybrid data plane — slice 2)

**As a** tenant whose model is latency-critical and/or must stay on my network
**I want** to run the proxy next to my model and keep the cloud as the control
plane
**So that** inference never leaves my network (minimal latency, no egress) while
I keep Inferon's dashboard, cost control, and telemetry

Slice 2 of option **C** (2026-10-11). Slice 1 — the cloud ingest backbone
(`POST /v1/edge/traces`) — is ITEM-052 (done 2026-10-12). This is the edge half
and its control loop. Direct endpoint (A) is ITEM-051; the cloud edge connector
(B) is deferred for latency.

**Acceptance Criteria:**
1. The proxy supports an **edge mode** serving the OpenAI-compatible surface on
   the customer's network against a **local** provider, resolving `base_url`
   directly (no cloud hop, no relay).
2. A tenant enrolls an edge node with a revocable **edge token**; the node syncs
   the tenant's **config** (model selection, routing/fallback, budget, guard,
   alert thresholds) and its **client key hashes** so local auth matches the
   cloud.
3. The edge pushes **batched telemetry** outbound via `POST /v1/edge/traces`
   (ITEM-052) under the node credential; the cloud writes it through the existing
   sink so the dashboard, alerts, and billing are correct.
4. Only small, batched control/telemetry traffic leaves the network — **inference
   payloads never traverse the cloud**.
5. Config changes made in the dashboard propagate to the edge without a redeploy;
   revoking the edge token stops the node.
6. Failure is explicit: cloud unreachable → the edge keeps serving (last config,
   buffered telemetry) and reports its state on reconnect.
7. The dashboard labels nodes (edge vs cloud) with last-seen health.

**Technical Notes:**
- **Edge mode:** `inferon --edge --control-url … --enrollment-token …`; reuse
  `build_router`/`AppState` with local stores (in-memory or a local Postgres).
- **Config sync:** a versioned snapshot (`GET /v1/edge/config`) + edge polling,
  or a control channel. The edge needs key **hashes**, routing/budget/guard
  config, and model selection — none of the secrets it does not already hold
  locally.
- **Enrollment:** edge token → node credential (new `edge_nodes` table:
  tenant, token hash, last_seen, config_version); revocation is a row delete.
- **Telemetry shipping:** reuse `EdgeTrace` (ITEM-052) as the wire format;
  buffer locally and retry with backoff.
- **Observability:** label the node (edge vs cloud) and its last-seen time in the
  operator console.
- **Security:** TLS; the provider key is a local row and never leaves the
  network.
- **Economics:** lowest-latency path, no cloud egress for the completion body;
  the trade-off is customer-run infrastructure.

**Source:** 2026-10-11 customer-hosted connectivity decision — see
[raw/llm-inference/2026-10-11-customer-hosted-connectivity-decision.md](../../raw/llm-inference/2026-10-11-customer-hosted-connectivity-decision.md)
and [wiki/llm-inference/customer-hosted-llms.md](../../wiki/llm-inference/customer-hosted-llms.md).
