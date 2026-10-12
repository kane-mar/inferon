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
