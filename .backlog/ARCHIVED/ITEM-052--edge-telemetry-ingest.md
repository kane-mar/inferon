## ITEM-052: Edge telemetry ingest (hybrid data plane — slice 1)

**As a** tenant running the proxy on-prem next to my model
**I want** its traces to reach Inferon's cloud control plane
**So that** my dashboard, cost control, alerts, and billing stay correct even
though inference never leaves my network

Slice 1 of the hybrid data plane (option **C**, 2026-10-11): the cloud-side
**backbone**. Slice 2 — the edge node mode, enrollment, and config sync — is
ITEM-065 (split 2026-10-12).

**Acceptance Criteria:**
1. `POST /v1/edge/traces` accepts a **tenant-authenticated** batch of edge traces.
2. The batch is queued through the **existing telemetry pipeline**, so the worker
   recomputes cost + waste centrally and persists via `PostgresTraceSink`; edge
   traffic then shows up in the dashboard, alerts, and billing exactly like
   local traffic.
3. The tenant is taken from the credential, **never the request body**; the
   edge's own cost/waste claims are ignored (the cloud is authoritative).
4. An oversized batch is refused with a clear typed error; missing or invalid
   credentials are 401.
5. Back-pressure is explicit: the response reports `{accepted, dropped}`.

**Close-out (2026-10-12):**
- `src/edge.rs`: `EdgeTrace` / `EdgeBatch` DTOs and `EdgeTrace::into_message`
  (tenant-scoped, cost/waste zeroed); route `POST /v1/edge/traces`; batch cap
  `MAX_EDGE_BATCH = 1000`; `accepted`/`dropped` from `try_send` back-pressure.
- Rust **236 → 239** tests, clippy clean. Commit `427053d`; CI `38248582733`
  (test+web+deploy) green.
- **Live**: a 2-trace batch → `202 {"accepted":2,"dropped":0}`, both persisted
  with cloud-recomputed cost + cache savings (`deepseek-flash` 1000/100/800 →
  $0.000116, saving $0.000170); unauthenticated → 401.

**Source:** 2026-10-11 customer-hosted connectivity decision — see
[raw/llm-inference/2026-10-11-customer-hosted-connectivity-decision.md](../../raw/llm-inference/2026-10-11-customer-hosted-connectivity-decision.md)
and [wiki/llm-inference/customer-hosted-llms.md](../../wiki/llm-inference/customer-hosted-llms.md).
