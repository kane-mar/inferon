# Inferon Project Direction, 2026-08-30

> Source: Pasted by user (Kane Mar); recorded decisions, no public URL
> Collected: 2026-08-30
> Published: 2026-08-30

Recorded. Four decisions locked in:

1. **Docker everywhere** — every component (Rust/Axum proxy, background workers, Postgres, Redis, training/serving) ships as a container. Deployment unit = image.
2. **Postgres** — this resolves the wiki's Disputed telemetry block (wiki/llm-inference/aio-proxy-overview.md) toward PostgreSQL (already the 2-of-3-source majority; the roadmap's "SQLite/PostgreSQL" is now just PostgreSQL).
3. **Working name: inferon** — matches the roadmap; the AIO-Proxy name in the original spec is retired.
4. **Default AI: DeepSeek** — new frontier default; the spec's cost table (gpt-4o $2.50/$10.00, gpt-4o-mini $0.15/$0.60, default $3.00/$15.00) and routing defaults (claude-3-5-sonnet fallback) are source-documented but the live default is DeepSeek — so the cost table and `frontier_fallback_model` need DeepSeek rows when we build.
