## ITEM-119: Protecting the bill and the context (budgets, limits, fallback, caching, pruning)

**As a** workspace owner
**I want** a cap that protects me without blocking real traffic, a chain that fires,
and caching or pruning that I can measure before I trust
**So that** cost control does not become a second incident

**Slices from ITEM-113.** Four configuration pages under `guides/config/`:

1. `05-budgets-and-rate-limits.md` — the monthly cap, the cheap fallback at 85%, per-key
   RPM/TPM, what a client sees at the edge (`Retry-After`, `X-RateLimit-*`), and how to
   pick numbers from real traffic instead of guessing.
2. `06-fallback-chain.md` — the ordered chain, the four per-step triggers (transport,
   429, 5xx, checker), why a 4xx is never retried, and what the client gets when the
   chain is exhausted.
3. `07-prompt-caching.md` — why the audit says "not cached", the cache-hit price, what
   makes a prompt cacheable, and where the saving appears.
4. `08-pruning-and-comparison.md` — context pruning, the deterministic holdout, how to
   read the two arms, and the automatic 5-point rollback.

**Acceptance Criteria**
1. Each page states the default, the range and the effect of the setting, and links to
   the dashboard page that holds it.
2. Budget and limit behaviour is shown with a typed refusal (`429` with
   `Retry-After`), and the fallback page shows a real stepped trace.
3. The caching page answers "why is it not cached" with the actual conditions, not a
   list of guesses.
4. `python3 scripts/check-guides.py` passes; `Last verified` carries the date.
5. Live on the durable host.

**Source:** ITEM-113 breakdown, 2026-11-05.
