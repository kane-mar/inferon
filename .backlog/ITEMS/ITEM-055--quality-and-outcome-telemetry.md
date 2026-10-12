## ITEM-055: Quality & outcome telemetry

**As a** tenant (and as the router/distillation loop)
**I want** a per-request signal of whether the response was actually usable
**So that** cost and latency can be weighed against quality, and routing can
prove a cheaper model is *good enough*

Today the proxy knows what a request **cost** and how **fast** it was, but not
whether it **worked**. `waste_detected` only flags linter rules; the schema guard
only catches schema violations. Refusals, empty completions, and bad answers are
invisible. This is the missing input for ITEM-029/030 (OmniRouting needs a
quality-gap signal) and for any cost/quality trade-off claim.

**Acceptance Criteria:**
1. Each trace records an **outcome**: at minimum `ok` (2xx + non-empty
   completion) vs `empty`/`refused`/`error`, derivable without the client.
2. A **client quality signal** can be attached out-of-band (thumbs up/down or a
   `quality` field on a follow-up endpoint) without changing the OpenAI
   compatibility of the inference path.
3. The dashboard surfaces a **quality view**: success rate, empty/refusal rate,
   guard-fallback rate, and (when supplied) thumbs, overall and by model.
4. The quality signal is available **per model and per system-prompt cluster**, so
   "this cheap model matches the frontier model for this task" is answerable with
   numbers, not opinion.
5. Quality is **not** required to use Inferon: a tenant that never supplies a
   signal still sees the derived `ok`/`empty`/`error` outcome.

**Technical Notes:**
- Add `outcome` (TEXT) and optionally `quality` (small JSONB: `{signal, score}`)
  to `traces` via an idempotent migration; extend `TraceMessage` + the sink.
- Derive `ok`/`empty` from the completion body already captured in
  `raw_completion` / the usage frame (no new upstream call).
- Guard fallback count already lives in `guard_event` / the guard aggregate —
  fold it into the quality view rather than duplicating.
- Keep the client signal optional and additive: `POST /v1/quality` keyed by trace
  id (or `X-Inferon-Trace-Id` response header) so SDKs need no change.

**Source:** 2026-10-12 metrics review — quality is the missing input for the
OmniRouting autonomy roadmap (ITEM-029/030).
