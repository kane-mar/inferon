## ITEM-080: Edge hybrid fallback — local first, explicit remote fallback

**As a** tenant running an edge node
**I want** a request the local model cannot serve (down, overloaded, 429, 5xx) to
fall back to an **explicitly configured** remote provider
**So that** an on-prem deployment degrades instead of failing — without ever
silently billing me or sending a prompt I did not opt into

**Context (2026-10-28):** the edge is local-only. `edge_runtime::run` inserts a
single provider (`--provider-url`), and `EdgeState` returns
`upstream_openai_key: None`, so `resolve_route`'s no-provider branch yields
`no_provider` rather than a cloud call. A dead local endpoint is a **502
`transport` with no failover** (seen live: `mac-studio.lan` → 502). The cloud
provider rows are not reachable from the node, by design (option C).

**Acceptance Criteria**
1. The node accepts an **explicit** fallback provider —
   `--fallback-provider-url`, `--fallback-provider-key-file`,
   `--fallback-provider-kind` — **off by default**. No cloud credential is ever
   fetched by the node; the fallback credential is a local secret like the
   primary one.
2. Fallback fires only on a **retryable** failure of the local attempt
   (transport, timeout, 429, 5xx). A 4xx from the local model is returned as-is
   (a malformed request will not succeed remotely either).
3. The fallback leg is traced **truthfully**: `is_local: false` (so it is priced
   normally), the local leg stays `0`, and the trace records which leg served the
   request (e.g. `routing_event { from: "local", to: "fallback" }`).
4. The tenant can **disable** fallback centrally (a flag in the edge config
   snapshot), so a node can be pinned to "local only" without editing its flags.
5. The prompt only leaves the network when fallback is enabled **and** actually
   used — visible in the trace and in the cost audit.

**Technical Notes**
- The edge provider store would hold two providers (local, fallback) with a
  deterministic order; `resolve_route` keeps preferring local.
- Reuse `build_model_chain` and the existing retry/fallback machinery; the new
  parts are a second credential and leg-aware tracing.
- Budget and guard apply to both legs; a fallback leg counts against the cap at
  its own price.
- Tradeoffs to document: **privacy** (the prompt leaves the network on that leg),
  **cost** (the fallback is not free), **latency** (a local timeout followed by a
  remote retry). Default off, and the trace must say which leg ran.
