## ITEM-021: Self-Healing Schema Guard (Slice 4)

**As a** developer running cheaper/smaller models in production
**I want** Inferon to guarantee my responses are schema-valid even when the cheap model drifts
**So that** I get the cost of a small model with none of the production risk

**Acceptance Criteria (refined 2026-09-05):**
1. Guard activates only when the request carries a JSON-schema contract (`response_format.json_schema.schema`) **and** the request is routed to a cheap tier (the ITEM-015 downgrade target, e.g. deepseek-v4-flash). Pure-passthrough traffic is byte-for-byte unchanged — zero behavior change for existing tenants
2. Streamed content from a guarded model is validated: JSON syntax first, then schema conformance (`jsonschema` crate over `choices[0].message.content`, per spec `schema_guard.rs`)
3. On any failure (invalid syntax, schema violation, guarded-upstream HTTP 5xx/connection crash) the bad response is discarded and the original prompt is transparently replayed to the tenant's frontier model — the client sees a latency spike on that request, never an error or a malformed body
4. Every guard event is recorded on the trace (outcome + failure reason per spec `fallback_circuit.rs`) and visible in tenant/operator views
5. Hot-path budget: buffered validation adds <5ms on guarded traffic (Slice 4 latency target); no added latency on unguarded traffic

**Refinement decisions (2026-09-05):**
- **Scoping — the guard needs no Slice 3 for its MVP.** The spec frames Slice 4 as guarding *distilled-model* traffic, but the failure modes (cheap model drifts on JSON) apply equally to the hosted cheap tier the Slice 2 router already sends traffic to. MVP guards hosted cheap-tier traffic; guarding distilled vLLM traffic activates later if Slice 3 ever lands. This resolves the dependency flag in the original capture: ITEM-021 is orderable *above* Slice 3.
- **Schema source**: `response_format.json_schema.schema` (OpenAI structured-outputs shape) — already visible in requests today (linter inspects `response_format`); no new client contract needed.
- **Fallback frontier model**: open question (below) — default proposal is the tenant's *original requested model* (pre-downgrade), which is the correct "frontier" in a DeepSeek-only world.

## Breakdown (2026-09-05 — sprintable slices, in pull order)

- **021a Buffered validation seam** — for guarded requests, buffer streamed SSE chunks in memory instead of piping through; flush on `[DONE]`. Guarded only, so passthrough path untouched. AC: guarded stream buffers; unguarded stream remains zero-copy.
- **021b Schema compile + validate** — parse accumulated content (`serde_json`), compile the request schema (`jsonschema`), validate; classify failure reasons (invalid syntax / contract malformed / schema violation / non-JSON container). AC: reason codes match spec `fallback_circuit.rs` list; unit tests with known-good + known-bad fixtures (TDD).
- **021c Transparent fallback circuit** — on failure: discard buffered response, replay original prompt (original model + tenant upstream key) to the frontier, stream the replacement; exactly one trace for the request recording the fallback. AC: client receives one valid response; mid-stream client abort during the buffered phase emits the ITEM-009 truncated trace.
- **021d Guard telemetry** — guard outcome + reason on the trace (migration if needed); tenant dashboard + operator views show guard/fallback counts per model; negative samples tagged for a future DPO dataset (queryable via the existing prompt-hash shape). AC: fallback rate per model visible; zero unguarded traces mutated.
- **021e Activation + regression safety** — default-on for cheap-tier+response_format traffic; per-tenant opt-out flag on `tenant_configurations`; extend smoke.sh with a guarded-failure scenario (dead guarded upstream → fallback 200); TTFT/latency check vs the <5ms budget on the durable host.

**Gates / open decisions (PO):**
1. Fallback frontier model: original requested model (default) vs an explicit configured frontier per tenant
2. Buffer-then-validate (spec) vs incremental streaming validation — incremental keeps TTFT flat but doubles validation complexity; MVP = spec behavior (buffer)
3. Default guard scope: all cheap-tier routes vs opt-in per tenant until ITEM-018 brings real traffic shapes

**Technical Notes:**
- Source: `wiki/llm-inference/inferon-slice-engineering-spec.md` Slice 4 + roadmap Slice 4; failure-reason list and crate choices from the spec's code-level details
- Extends the buffering introduced for trace-on-abort (ITEM-009) rather than fighting it; fallback replay reuses the tenant-key upstream path, not the linter
