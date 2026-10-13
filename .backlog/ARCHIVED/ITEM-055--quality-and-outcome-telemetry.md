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

## Close-out (2026-10-13)

- `src/quality.rs`: `Outcome {ok, empty, refused, error, truncated}` derived from
  the status + body the proxy already holds — OpenAI and Anthropic envelopes,
  both JSON and SSE — with no extra upstream call and no client dependency
  (12 unit tests: blank content, tool calls, content_filter, Anthropic
  `stop_reason: refusal`, streaming deltas, non-2xx).
- Persistence: `traces.outcome` (migration 025) via `TraceMessage`; edge traces
  may carry their own outcome and fall back to a status-derived one.
- `X-Inferon-Trace-Id` on completion responses; `POST /v1/quality` attaches an
  optional client thumbs up/down, upserted into `trace_quality` keyed by trace id
  so it never races the async trace insert.
- `GET /v1/dashboard/api/quality`: overall success/empty/refusal/error/
  guard-fallback rates + thumbs (`unclassified` for pre-025 rows), by model and
  by system-prompt cluster.
- Web: `/dashboard/quality` page + nav entry.
- Rust **252 → 261** tests, clippy clean; tsc/eslint/i18n green. Commits `7a4d91c`
  + `203d45b`; CI `38385227716` (test+web+deploy) green.
- **Live**: a real `deepseek-flash` request returned `X-Inferon-Trace-Id`, the
  trace recorded `outcome = ok`; `POST /v1/quality {"signal":"down"}` answered 202
  and `GET /api/quality` reflected `thumbs_down = 1` for that model, with
  per-model success rates. Historical rows read `unclassified` (outcome was never
  recorded before 025).
