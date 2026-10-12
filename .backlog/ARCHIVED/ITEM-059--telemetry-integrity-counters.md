## ITEM-059: Telemetry integrity counters

**As an** operator
**I want** to know the telemetry pipeline isn't silently losing or mispricing data
**So that** the cost and usage numbers I show tenants are trustworthy

Two silent failure modes exist: (1) when the telemetry channel is full the trace
is **logged and dropped** (`telemetry channel full; trace dropped`) with no
counter, so spend/usage is undercounted; (2) requests for models with no known
price fall back to a default price with only a log.

**Acceptance Criteria:**
1. Counters for **traces emitted vs persisted vs dropped** (and dropped by
   reason: channel full / channel closed), exposed on an operator endpoint.
2. A count of requests served with an **unpriced/estimated model**, so cost
   accuracy is measurable rather than assumed.
3. An alert (or a visible warning) when the drop rate exceeds a threshold, so
   silent undercount becomes visible.
4. The operator view reconciles the numbers: the dashboard can state "N traces
   dropped in this window — totals are a lower bound".

**Technical Notes:**
- Increment atomic counters at the `try_send` seam in `lib.rs`; expose via an
  operator-only JSON endpoint alongside the existing operator summary.
- The unpriced-model counter hooks the existing `warn_if_unpriced` path in
  `cost.rs`.
- Keep counters in-process (they reset on deploy) unless persistence is trivial;
  the value is a live health signal, not historical billing.

**Source:** 2026-10-12 metrics review — you can't trust numbers you can't
reconcile.

## Close-out (2026-10-12)

- `src/metrics.rs`: process-global counters for `traces_queued`, `traces_persisted`,
  `traces_dropped_full` / `traces_dropped_closed` (+ derived `lost`), and
  `unpriced_models`.
- Incremented at the send seam (`send_trace` and the edge ingest), in the Postgres
  sink (once per committed batch), and in `cost::warn_if_unpriced`; exposed on the
  operator-only `GET /v1/dashboard/api/operator/metrics`.
- Rust **239 → 240** tests, clippy clean. Commit `5e20df6`, CI `38253622143`
  (test+web+deploy) green.
- **Live**: the endpoint returns the counters, unauthenticated → 401, and one real
  request moved `traces_queued` 1→2 and `traces_persisted` 1→2 with `lost: 0`.
