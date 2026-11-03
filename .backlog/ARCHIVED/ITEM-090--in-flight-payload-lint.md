## ITEM-090: In-flight payload lint (advisory)

**As a** tenant
**I want** the proxy to tell me, before the call, that this request is wasting
tokens
**So that** I can fix the payload — not read about it in yesterday's dashboard

**Claim (2026-10-30 brief).** "In-flight request linting. Inspecting payload bloat
before it leaves the perimeter: flagging redundant system prompts repeated across
turns; detecting schema bloat or token-hungry boilerplate."

**Current state (2026-10-30).** Rules A/B/C run in the **background batch worker**
(`linter::lint`, after the trace is written): over-provisioned model, repeated
≥4,000-token uncached system prompt, frontier model on simple text. The hot path
only scores complexity (for downgrades). Nothing inspects the payload pre-dispatch.

**Gap.** Post-mortem only; no pre-flight finding, no schema-bloat or boilerplate
check, no per-request visibility.

**Acceptance Criteria**
1. Pre-dispatch checks on the parsed payload, producing findings such as:
   **repeated system prompt** (same hash seen within a short window),
   **oversized schema** (tool/parameter definitions beyond a byte budget),
   **boilerplate** (a large few-shot/instruction block repeated verbatim across
   requests), and **redundant turns** (duplicate messages in the same conversation).
2. Findings are recorded on the trace (`request_lint`, additive) with the estimated
   wasted tokens and USD using the existing cost engine, and are visible to the
   client (e.g. an `X-Inferon-Lint` response header) and in the dashboard.
3. **Advisory only**: the payload is forwarded unchanged; the model may still be
   downgraded by the existing router. Rewriting is ITEM-091.
4. Cost: the pass is bounded and benchmarked (target stated at pull; no additional
   DB round trip beyond ITEM-092's snapshot).
5. Per-tenant kill switch; a false-positive report path (a finding can be dismissed).
6. Findings never fail a request — a lint error is logged and skipped.

**Technical Notes**
- Reuse the complexity scorer's traversal for the payload walk.
- The "repeated prompt" check needs the prompt cache from ITEM-092/089; today's
  `system_prompt_hash` aggregate is a DB read, too slow per request.
- Keep the post-hoc linter: it catches distribution-level waste the pre-flight check
  cannot (e.g. cache misses across a day).

**Ranking note.** Rank **7** of 22 · V3.5/E2.5 — the pre-flight half of the linting claim; after 092 because it needs the prompt cache.