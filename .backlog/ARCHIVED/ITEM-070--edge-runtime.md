## ITEM-070: Edge runtime (`--edge` mode)

**As a** tenant running the proxy on-prem
**I want** a single binary I can run next to my model
**So that** inference never leaves my network while I keep Inferon's dashboard,
cost control, and telemetry

Split from ITEM-065 (2026-10-14): the cloud control plane is delivered
(`edge_nodes`, enrollment/revoke, versioned `GET /v1/edge/config`, node-authed
`POST /v1/edge/traces`). This item is the customer-run half.

**Acceptance Criteria:**
1. `inferon --edge --control-url … --node-token …` serves the OpenAI-compatible
   surface on the customer's network against a **local** provider (resolving
   `base_url` directly — no cloud hop, no relay).
2. The node **polls** `GET /v1/edge/config`, applies the snapshot (key hashes,
   model selection, routing/fallback, budget/guard/router, active policy), and
   re-syncs on change **without a redeploy**.
3. Traces are **buffered locally** and shipped in batches via
   `POST /v1/edge/traces` with retry/backoff; **inference payloads never leave
   the network** — only small telemetry/control traffic does.
4. Local auth uses the synced client **key hashes**; the provider key is a local
   secret (never fetched from the cloud).
5. Failure is explicit: cloud unreachable → the node keeps serving with the last
   config and buffered telemetry, and reports its state on reconnect.
6. Reusing the cloud's app state with local stores (in-memory or a local
   Postgres) — `build_router`/`AppState` — so the surfaces stay identical.

**Technical Notes:**
- Reuse the `EdgeTrace` wire format (ITEM-052) for shipping.
- The re-sync loop is `GET /v1/edge/config` + a version compare; a 401 means the
  node was revoked and it should stop.
- Keep the local buffer bounded (drop-oldest) so an offline node cannot grow
  without limit.

**Source:** 2026-10-11 customer-hosted connectivity decision (option C);
ITEM-065 split 2026-10-14.

## Close-out (2026-10-15)

- `inferon --edge --control-url … --node-token … --provider-url … --provider-key …`
  (parsed in `main.rs`) runs the node; `edge_runtime::run` builds an edge
  `AppState` and serves the existing `build_router`.
- `EdgeState` (a `RwLock<EdgeConfig>`) implements the tenant, budget, routing,
  model and policy stores over the **synced snapshot**, so local auth uses the
  cloud's client-key hashes and routing/budget/guard/policy are the cloud's
  config. The local provider credential is a local flag — never fetched.
- A sync loop polls `GET /v1/edge/config` and re-applies on a version change; a
  **401 (revoked) stops it**, and a failed poll keeps serving the last config.
- Telemetry is buffered locally and shipped by `RemoteTraceSink` to
  `POST /v1/edge/traces` (bounded, drop-oldest; a 401 clears the buffer). Only
  small config/telemetry traffic leaves — **inference payloads never do**.
- Rust **301 → 305** tests, clippy clean. Commit `d96d72f`; CI `38673276255`
  (test+web+deploy) green.
- **Live**: started an edge node against the cloud (tenant kane-mar) with the
  in-image mock as its local provider. A request authenticated **locally**,
  reached the mock, returned **200** (SSE), and the trace reached the cloud
  (`local-llm | 200 | provider attributed`). The node and its client key were
  revoked afterwards.
