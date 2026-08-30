# Inferon Decision: Budget Spend Counter (2026-08-30)

> Source: Recorded product decision by Kane Mar via pi session, 2026-08-30
> Collected: 2026-08-30
> Published: 2026-08-30

Decision: the budget circuit-breaker spend counter lives in **PostgreSQL for the MVP** — an atomic UPDATE on `tenant_configurations.current_spend_usd` read and written on the routing hot path — not Redis. Redis (fast-path counter with async reconciliation to Postgres) is the documented scale-out path for when per-request Postgres writes become the bottleneck (typically 10k+ req/s). Counter values are stored as **integer cents, not float USD**. This resolves the Redis spend-counter format dispute between the two implementation documents (`HINCRBY tenant:{id}:spend {cost_in_cents}` vs `tenant:{id}:monthly_spend` float USD).

Rationale: Postgres is already the source of truth for spend (`current_spend_usd`), so a separate Redis counter would create two sources of truth with drift risk; a single atomic UPDATE is durable and fast enough at MVP scale; float USD in an f64 Redis value risks rounding errors — integer cents avoids that in any store.
