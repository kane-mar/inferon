## ITEM-075: Edge serves the local provider's own models

**As a** tenant running an edge node next to my own model
**I want** the node to advertise and route the model ids my local server actually serves
**So that** I can point it at LM Studio / Ollama / vLLM without pretending my
on-prem model is a cloud catalog entry

**Found:** 2026-10-15, running the first edge node against LM Studio
(`mac-studio...ts.net:1234`) — reported as "edge cannot see the local model".

**Problem:** `edge_runtime::run` registered `config.models` (the tenant's **cloud**
catalog — `deepseek-flash`, `deepseek-v4-pro`) against the **local** provider. A
local server serves its own ids, so either the cloud id is forwarded and 404s
upstream, or the local id misses `providers_for_model` and is rejected with
`no_provider`. The node was effectively unusable with a real local server.

**Acceptance Criteria**
1. On startup the node reads `GET <provider-url>/models` and registers the local
   ids against the local provider.
2. A failed or empty local read falls back to the cloud catalog (keeps serving;
   never fails to start).
3. The sync loop re-discovers, so a model loaded/unloaded locally appears without
   a restart.
4. `GET /v1/models` on the node lists the local ids.

## Close-out (2026-10-15)

Implemented and verified with a real local OpenAI-compatible provider (a mock
on `127.0.0.1:9099` advertising `local-test-model`) driven through the actual
edge node against the deployed cloud.

- `providers::parse_model_ids` + `providers::fetch_models` moved into the provider
  domain; the dashboard probe and the edge node share them, so the node no longer
  registers the cloud catalog against the local provider.
- `edge_runtime::local_models` reads `GET <provider-url>/models` at startup and on
  each config sync; a failed or empty read falls back to the cloud catalog.
- **AC1/AC4**: the node's `GET /v1/models` returned `{"id":"local-test-model"}`;
  `POST /v1/chat/completions` with `local-test-model` → **200** from the local server.
- **AC2**: a dead provider URL falls back (test + the node still starts).
- **AC3**: the sync loop re-discovers on every config change.
- Telemetry reached the cloud: `local-test-model | 200 | ok | 5/3 tokens` with the
  provider attributed, at 2026-10-15 08:52:00Z.
- Rust **306 → 308** tests, clippy clean. Commit `1d07081`; CI `38708530820` green.
