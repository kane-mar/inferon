# Refinement Log

## 2026-09-05 — ITEM-014 archived; backlog empty

- **Hostname decision**: live endpoint is `https://inferon.agilefederation.com/v1` — a subdomain of the existing company domain (one A record in DNSimple, no registrar delegation). The sav.com/delegation path for api.inferon.dev was abandoned (registrar action blocked); inferon.dev remains parked at sav.com. This supersedes the Vultr-DNS-zone + NS-delegation approach noted 2026-09-04 (whose Vultr-side zone also turned out never to have been created).
- **ITEM-014 (durable deployment) archived — all ACs verified**: HTTPS live on the durable host (Let's Encrypt cert, public chat + tenant/operator dashboards exercised over the public URL, auth boundary 401/403 intact), restart test keeps real data (2 traces + rule-B counts), tenant minted via seed-tenant.sh with a live DeepSeek master key, first Vultr snapshot taken (4dca04b7), cost ~$5-6/mo acknowledged.
- **Slice 1 + 2 backlog is now fully delivered and archived.**
- **Backlog empty (Rule 5)** — replenish. Candidate order (Slice 3 moved to the END by product owner, 2026-09-05): fine-grained live-product items first (real developer onboarding / first external tenant, dashboard polish, anomaly alerts — these get full stories when the backlog is written), then Slice 4 self-healing schema guard, then Slice 5 autonomous OmniRouting, with **Slice 3 automated LoRA distillation last** (chunky at the bottom; needs breakdown only if it ever reaches the top). Roadmap reference: wiki/llm-inference/inferon-product-roadmap.md.

## 2026-09-04 — Slice 1 complete; backlog replenished

- **Archived**: ITEM-001..007 (Slice 1: docker scaffold, schema, zero-copy proxy, tenant auth, telemetry, cost linter, dashboard) — all ACs verified live on disposable Vultr hosts; 45 tests green.
- **Refinement session**: backlog was empty (Rule 5 — flagged). Replenished from the documented gap shortlist + the roadmap's next slice.
- **New items (single-ranked, small→large)**: ITEM-008 DeepSeek real pricing (dashboard $ are wrong today), ITEM-009 traces on stream abort (data loss), ITEM-010 rule B counts survive restart, ITEM-011 runbook, ITEM-012 smoke coverage, ITEM-013 operator view (flagged: conflicts with ITEM-007 AC5 tenant isolation — needs an operator auth mechanism), ITEM-014 durable deployment (first always-on spend — business decision, break down when it rises), ITEM-015 + ITEM-016 Slice 2 (heuristic router, Postgres budget breaker — left chunky per progressive granularity).
- **Product goal updated** for the next horizon: durable api.inferon.dev + automatic cost protection.
- **Items deliberately not backlogged**: future Slices 3–5 (distillation/serving/autonomous) — too large and low-probability; will be captured when relevant.

## 2026-09-04 (later) — ITEM-008 archived; ITEM-017 captured

- ITEM-008 (DeepSeek real pricing) done + archived after live verification.
- **Captured ITEM-017** (cache-hit-aware costing) — flagged follow-up to ITEM-008: DeepSeek auto-caches long prefixes and bills cached input at ~$0.007 vs $0.22 miss, so the single miss-rate row overstates repeated-prompt spend up to ~30×. Ranked #3 (after the 009 trace-data-loss bug, before 010) because it affects every cached DeepSeek trace's reported cost. `cached_prompt_tokens` is already stored per trace (ITEM-005), so this is a `calculate_cost` signature + worker call-site change only.
