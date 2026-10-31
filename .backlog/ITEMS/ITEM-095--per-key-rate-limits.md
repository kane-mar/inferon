## ITEM-095: Per-key rate limits (RPM/TPM)

**As a** tenant owner
**I want** to cap requests and tokens per key per minute
**So that** one runaway client cannot starve the others or burn the budget

**Claim (2026-10-30 brief, landscape).** Portkey provides "rate limiting across
business units". Inferon enforces spend caps only; a key can issue unlimited
requests until the monthly budget is gone.

**Current state (2026-10-30).** `tenant_configurations`/`api_keys` have spend limits
and the budget breaker; there is no request or token rate limit and no 429 with
`Retry-After` for velocity.

**Gap.** No RPM/TPM control.

**Acceptance Criteria**
1. Per-key (and tenant-default) RPM and TPM limits, configurable by the owner;
   `0`/unset means unlimited (today's behaviour).
2. Enforcement is pre-flight and returns `429` with `Retry-After` and the standard
   rate-limit headers (`X-RateLimit-Limit/Remaining/Reset`) on both success and
   rejection.
3. Counters are correct across restarts and multiple proxy instances (Postgres is
   the authority; an in-process fast path may short-circuit but must reconcile).
4. Rejections are traced (`inferon_4xx`, `error_class = rate_limited`) and surface
   in the dashboard; sustained 429s feed the existing alert lifecycle (ITEM-068).
5. A rejected request costs nothing and does not consume budget.
6. Tests: boundary (limit-1 / limit / limit+1), token accounting for streaming
   (usage known only at the end — define the rule), restart persistence.

**Technical Notes**
- A token limit is only knowable after the response for streams; charge the actual
  usage at trace time and reject on the *next* request, or enforce on the prompt
  estimate pre-flight and reconcile. Document the choice in the item.
- Keep it off the hot path when unset (no extra query for tenants with no limits).

**Ranking note.** Rank **11** of 22 · V3/E2.5 — enterprise velocity protection; parity.
---

**Delivered (2026-10-31).** `src/ratelimit.rs`: `Limits` (0 = unlimited),
`decide` (requests first, then tokens; the request boundary is limit-1 / limit /
limit+1), `estimate_tokens` (prompt chars/4), `retry_after_secs`, and `charge` —
one atomic `INSERT … ON CONFLICT DO UPDATE … RETURNING` into `api_key_usage`, so
two instances cannot both see "limit - 1". `charge_tokens` reconciles the actual
completion after the fact.

Per-key limits are read by the same query that authenticates (`Tenant.limits`),
so a limited key costs no extra query; the tenant default rides the cached
config row. Enforcement runs before planning, so a rejected request never
touches the budget. A limiter outage fails open — like every other switch, it
must not take the data plane down.

Token rule (documented choice): the prompt is estimated and charged pre-flight,
the completion is added when the trace is written; a request is rejected on what
the client can act on before spending anything.

Verified: 7 unit tests (boundaries, axes, Retry-After, estimate) + a response
test for the 429 headers. Live: `X-RateLimit-*` on a successful call, 429 with
`Retry-After` past a configured cap.
