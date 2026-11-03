## ITEM-093: Conversation and transaction grouping

**As a** tenant running agentic loops
**I want** the 10–20 calls of one user transaction grouped and costed together
**So that** the unit I optimise is the transaction, not an isolated request

**Claim (2026-10-30 brief).** "In high-throughput, agentic multi-turn loops (where
10–20 internal LLM calls happen per user transaction)" — a gateway's overhead and
cost analysis must be per transaction, not per request.

**Current state (2026-10-30).** Traces are per request. `X-Inferon-Trace-Id` links a
response to its trace for quality thumbs; there is no session/transaction id, so
per-transaction cost, turn count and retry overhead cannot be computed.

**Gap.** No grouping key and no per-group read model.

**Acceptance Criteria**
1. A client may send `X-Inferon-Session-Id` (opaque, tenant-scoped, validated and
   length-capped); absent means today's behaviour (one request per group).
2. Traces store the session id (additive, indexed with tenant + time).
3. A read model reports per session: turns, models, providers, total cost, router/
   fallback savings, errors by class, p95 latency, and the wall-clock span.
4. The dashboard shows transaction-level cost distribution (e.g. p50/p95 cost per
   session) alongside the per-request view.
5. Session ids never cross tenants and are not exposed in operator views beyond
   aggregates.
6. Optional: a session id flows into dataset grouping so multi-turn examples can be
   exported as a conversation, not as isolated turns.

**Technical Notes**
- Prefer a header over a body field: the payload is forwarded upstream and must not
  change shape. Strip the header before forwarding.
- Do not aggregate into a new table per request; a session is a read model over
  traces until volume proves otherwise.
- Reject malformed ids with a typed 400? No — ignore and trace it; grouping is
  best-effort and must never fail a request.

**Ranking note.** Rank **6** of 22 · V3/E1.5 — the cheapest item after 085: names the transaction, enabling agentic-loop economics and multi-turn examples.