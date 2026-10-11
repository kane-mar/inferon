## ITEM-052: Hybrid data plane (on-prem proxy + cloud control plane)

**As a** tenant whose model is latency-critical and/or must stay on my network
**I want** to run the proxy next to my model and keep the cloud as the control
plane
**So that** inference never leaves my network (minimal latency, no egress) while
I keep Inferon's dashboard, cost control, and telemetry

This is option **C** of the customer-hosted connectivity decision (2026-10-11):
split the control plane (cloud) from the data plane (edge). The customer runs the
same Rust binary on-prem; the cloud keeps auth, configuration, and telemetry
aggregation. Direct endpoint (A) is ITEM-051; the cloud edge connector (B) is
deferred for latency.

**Acceptance Criteria:**
1. The proxy supports an **edge mode**: it serves the OpenAI-compatible surface
   on the customer's network against a **local** provider, resolving
   `base_url` directly (no cloud hop, no relay).
2. A tenant enrolls an edge node with a revocable **edge token**; the node syncs
   the tenant's **config** (model selection, routing/fallback, budget, guard,
   alert thresholds) and its **client key hashes** so local auth matches the
   cloud.
3. The edge pushes **batched telemetry** (traces/cost) outbound to the cloud
   under a node credential; the cloud writes it through the existing sink so the
   dashboard, alerts, and billing are correct.
4. Only small, batched control/telemetry traffic leaves the network — **inference
   payloads never traverse the cloud**.
5. Config changes made in the dashboard propagate to the edge without a redeploy;
   revoking the edge token stops the node.
6. Failure is explicit: cloud unreachable → the edge keeps serving (last config,
   buffered telemetry) and reports its state on reconnect.

**Technical Notes:**
- **First slice (backbone):** the cloud **telemetry-ingest** endpoint
  (`POST /v1/edge/traces`, tenant-authenticated) that accepts trace batches and
  flushes them through `PostgresTraceSink`; everything else builds on it.
- **Edge mode:** `inferon --edge --control-url … --enrollment-token …`; reuse
  `build_router`/`AppState` with local stores (in-memory or a local Postgres).
- **Config sync:** versioned snapshot (`GET /v1/edge/config`) + edge polling, or
  a control channel. The edge needs key **hashes**, routing/budget/guard config,
  and model selection — none of the secrets it doesn't already hold locally.
- **Observability:** the dashboard labels the node (edge vs cloud) and its last
  seen time; the operator can see per-node health.
- **Security:** edge token → node credential; TLS; the provider key never leaves
  the network (it is a local provider row).
- **Economics:** this is the lowest-latency path and avoids cloud egress for the
  completion body; the trade-off is customer-run infrastructure.

**Source:** 2026-10-11 customer-hosted connectivity decision — see
[raw/llm-inference/2026-10-11-customer-hosted-connectivity-decision.md](../../raw/llm-inference/2026-10-11-customer-hosted-connectivity-decision.md)
and [wiki/llm-inference/customer-hosted-llms.md](../../wiki/llm-inference/customer-hosted-llms.md).
