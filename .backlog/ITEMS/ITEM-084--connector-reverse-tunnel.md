## ITEM-084: Connector / reverse tunnel (staged — do not pull)

**Status:** **staged.** Unpark only if a customer refuses **both** a reachable
endpoint (ITEM-051/082) **and** Docker on their own host (ITEM-083).

**As a** customer who cannot expose an endpoint and will not run the stack
**I want** an outbound-only agent that connects their model to Inferon
**So that** their model is reachable without inbound firewall changes

**Why it is staged (2026-10-28):** it is the most expensive of the three
connectivity options and the only one whose economics get *worse* the more it is
used — the prompt travels client → cloud → tunnel → on-prem model → back, so it
adds a cloud hop, adds tunnel latency, and gives up the privacy property a
customer presumably wanted. The reachable-endpoint path (already built) and the
self-hosted path (ITEM-083) both cover the same need for less.

**Acceptance Criteria (if ever unparked)**
1. An agent opens a single outbound WebSocket to the cloud and serves the
   OpenAI-compatible surface over it — no inbound port, no public DNS.
2. Streaming is framed end to end with backpressure and per-stream
   cancellation; a slow client must not stall the tunnel.
3. The agent authenticates as a tenant/node credential (same revocation model as
   edge nodes) and fetches no provider credential from the cloud.
4. Traces record the tunnel leg and its added latency; the dashboard shows it as
   the path taken.
5. The guide states the tradeoffs in the same breath as the setup: prompts
   transit the cloud, added latency, and it is only appropriate when neither
   a reachable endpoint nor self-hosting is possible.

**Technical Notes**
- Reuse the edge node's credential, config-sync, and telemetry-ingest surface
  (`/v1/edge/*`) rather than inventing a second enrolment path.
- Do not make it a routing target for cloud traffic; it is a transport for a
  customer's own model, chosen per provider.

**Source:** 2026-10-28 customer-hosted connectivity review — option B, staged.
