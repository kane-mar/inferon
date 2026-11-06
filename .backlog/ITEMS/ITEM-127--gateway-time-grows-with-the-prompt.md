## ITEM-127: Gateway time scales with the prompt — the sub-millisecond claim does not hold for real payloads

**As a** reader of the Runtime page
**I want** the gateway overhead to mean what the product claims
**So that** "sub-millisecond" is not contradicted by the dashboard 20 cm away

**Found 2026-11-06 during the UI review.** The Runtime page reported **gateway time
p50 = 1.2 s** for the workspace while the README, the landing page and ITEM-092 claim
*sub-millisecond* proxy overhead (the claim was verified on 41 small requests: p50
111 µs).

Measured on the live host, per request (`proxy_us`, the time from the start of the
request to just before dispatch):

| Prompt tokens | `proxy_us` | total |
|---|---|---|
| 35 (my probes) | 2.2–11 ms (first one 90 ms, cold) | ~340 ms |
| ~310,000 | 1.25 s – 2.2 s | 3.4 – 4.8 s |
| ~313,600 | **10.2 s** | 12.6 s |

So the number is not a regression over time — it is **proportional to payload size**.
The monthly p50 was 1.2 s because real traffic on this workspace sends ~300k-token
prompts.

**Where the time goes (whole-payload passes before dispatch, per request):**
- `read_json_body` → `serde_json::from_slice` of the whole body (unavoidable, it is the
  API shape);
- `preflight` (`crate::preflight::inspect`) walks every message;
- `redact_wire` walks and may rewrite the payload;
- `prune_payload` can rebuild it;
- `enforce_rate_limit` calls `estimate_tokens(meta.raw_prompt.to_string().len())` —
  **serialising the entire prompt to a String just to read its length**;
- `system_prompt_hash` serialises the system prompt.

**Acceptance Criteria**
1. `proxy_us` for a 300k-token prompt is under 100 ms (a 20× reduction), and the
   Runtime page's KPI states the payload size it was measured at, so "sub-millisecond"
   and "1.2 s" can never appear side by side without context.
2. No path serialises the whole prompt solely to measure it (length from the parsed
   value, or tracked while parsing).
3. A benchmark in `benches/` or an ignored test asserts the budget at 4 KB and 300 KB,
   so the claim is checked against a large prompt, not only a smoke request.
4. The Runtime page separates **gateway time by payload size** (or shows the p50 with
   the token window), because one number across 35-token and 300k-token traffic tells
   the reader nothing.
5. The README/landing claim is restated with the payload size it holds for, and
   ITEM-092's evidence line records the 300k case.

**Technical Notes**
- The biggest single win is `estimate_tokens`: it needs a character count, not a
  String. `serde_json::Value` cannot report its rendered length cheaply, but the body
  bytes' length is already in hand at parse time and is what matters for an estimate.
- `preflight`'s per-message walk is the second: it can stop early once every rule has
  fired, and the PII scanner already streams.
- Measure before and after with the same 300k-token payload; the trace carries
  `proxy_us` and `prompt_tokens`, so the comparison is one SQL query.

**Source:** UI review, 2026-11-06 (the Runtime page contradicted the product claim).
