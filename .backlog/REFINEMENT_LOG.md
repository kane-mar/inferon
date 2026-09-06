# Refinement Log

## 2026-09-06 (late) — OmniRouting pull reviewed; ITEM-028..030 staged with evidence

Engineering review after the ITEM-023..027 guard chain archived (all ACs,
smoke passed live on the durable host). ITEM-028 reached the top of the
pullable region and was pulled — the pull surfaced hard blockers, so 028..030
were returned to the backlog as **staged** (in-list, not pullable) with
measured evidence recorded in-item:

- **ort does not build on the dev toolchain**: ort 2.x (only non-yanked
  crates.io line) has no `x86_64-apple-darwin` ONNX Runtime prebuilt (build
  fails); ort 1.x is mass-yanked (v1.16.3 builds only as a git-tag dep).
  Every slice so far relied on the local cargo test loop — a Linux-only ort
  dependency breaks the TDD loop (host builds are ~30 min on the 1 vCPU host).
- **AC2 (<3 ms) not met by any real artifact, measured**: TaylorAI/bge-micro-v2
  quantized int8 (17 MB — the authentic micro-class model) = **7.4 ms** mean
  per (1,128) embed on this machine (Level3 opt, 4 intra threads);
  all-MiniLM-L6-v2 int8 (23 MB) = 13 ms. A sub-3 ms micro embedder is itself
  a distillation product — i.e. the parked Slice 3.
- **029 needs real task traffic**: today's corpus has no real task clusters;
  centroid building (AC1) and the AC4 quality gate would be vacuous — the
  refinement-log ordering question answers itself.
- **030 consumes 029**: replacing the working ITEM-015/016 protection with an
  epsilon schedule requires the 029 quality-gap signal to be valid.

**Unpark triggers (PO to choose)**: (1) PO re-baselines the 028 latency AC
and accepts a linux-gated build loop for ort code → pull 028 now; (2) Slice 3
(distillation) unparked → its micro artifact rides the 031/032 gate; (3) a
newer-CPU proxy tier measures the existing artifact <3 ms. Guard chain
023..027 complete; pullable region otherwise empty.

## 2026-09-06 — ITEM-033 captured (PO backlog review)

- **New item captured**: user accounts & logins (ITEM-033, feature, captured). Human-facing accounts + login — distinct from ITEM-004 (machine API-key auth, done) and ITEM-018 (out-of-band key minting). Placed at bottom of pullable region (large/unrefined). Open PO questions logged in-item: who logs in (external devs vs operators), account↔tenant model, self-serve vs invite signup, auth mechanism, and ranking vs the ITEM-018/019/020 cluster.
- **Q1 answered (PO)**: operator views **do** move to accounts — the 4 `/api/operator/*` JSON APIs (today authed by the shared `OPERATOR_API_KEY`) become account-authenticated, which also introduces the first human-facing operator screen. **Operator key sub-question answered**: the shared `OPERATOR_API_KEY` survives for scripting/machine access alongside accounts (accounts = humans; key = deploy scripts/cron/curl).
- **Q2–Q5 answered (PO, same session)**: account model = **tenants with multiple users (owner + members), accounts manage the tenant's API keys** (replacing operator-run `seed-tenant.sh` minting for humans); signup = **self-serve** (auto-provisioned tenant + key); mechanism = **OAuth with GitHub initially**; ranking = “least responsible moment” (read: least-disruptive position — large body of work stays low, not blocking the small pullable cluster). ITEM-033 still captured/unrefined; now clearly chunky enough to break down when it approaches the top.

## 2026-09-05 — ITEM-014 archived; backlog empty

- **Hostname decision**: live endpoint is `https://inferon.agilefederation.com/v1` — a subdomain of the existing company domain (one A record in DNSimple, no registrar delegation). The sav.com/delegation path for api.inferon.dev was abandoned (registrar action blocked); inferon.dev remains parked at sav.com. This supersedes the Vultr-DNS-zone + NS-delegation approach noted 2026-09-04 (whose Vultr-side zone also turned out never to have been created).
- **ITEM-014 (durable deployment) archived — all ACs verified**: HTTPS live on the durable host (Let's Encrypt cert, public chat + tenant/operator dashboards exercised over the public URL, auth boundary 401/403 intact), restart test keeps real data (2 traces + rule-B counts), tenant minted via seed-tenant.sh with a live DeepSeek master key, first Vultr snapshot taken (4dca04b7), cost ~$5-6/mo acknowledged.
- **Slice 1 + 2 backlog is now fully delivered and archived.**
- **Backlog empty (Rule 5)** — replenish. Candidate order (Slice 3 moved to the END by product owner, 2026-09-05): fine-grained live-product items first (real developer onboarding / first external tenant, dashboard polish, anomaly alerts — these get full stories when the backlog is written), then Slice 4 self-healing schema guard, then Slice 5 autonomous OmniRouting, with **Slice 3 automated LoRA distillation last** (chunky at the bottom; needs breakdown only if it ever reaches the top). Roadmap reference: wiki/llm-inference/inferon-product-roadmap.md.
- **Replenished (later same day)**: ITEM-018..020 added refined (stories + ACs), ITEM-021 (Slice 4) + ITEM-022 (Slice 5) added captured/chunky. Slice 3 not added — parked per PO. Commit 2d372e9.

## 2026-09-05 (later) — ITEM-021 + ITEM-022 refined

- Broke both roadmap items down from the engineering spec (`inferon-slice-engineering-spec.md` Slice 4/5 code-level details) into sprintable slices with mini-ACs (021a-e, 022a-e) + gates/open decisions.
- **Key finding — the Slice 3 dependency evaporates for the MVP of both**: 021's schema guard protects the *hosted cheap tier* the Slice 2 router already sends traffic to (spec only frames distilled traffic; failure modes identical), and 022's Lagrangian/embedding core routes between *hosted* frontier vs cheap tiers. Only the extensions (021: guard distilled vLLM traffic; 022d/e: evolution loop + PagedLoRA pool) need Slice 3. Both items are now orderable above distillation — consistent with the PO parking Slice 3.
- Decisions logged in-item: 021 schema source = response_format.json_schema.schema; fallback = original requested model (frontier in a DeepSeek-only world). 022 epsilon schedule verbatim from spec burn-ratio formulas (1.2/0.5 → 0.02/0.15/0.05), consuming ITEM-016's existing counters; embedding classifier additive over ITEM-015 heuristics.
- Open PO decisions flagged: fallback frontier config, buffer-vs-incremental validation, guard default scope, embedding-model artifact shipping (affects snapshot size), and whether 022b/c should wait for real traffic (018/020) before building.
- **Split into pullable items (same day, PO request)**: ITEM-021 → ITEM-023..027 (guard slices a-e), ITEM-022 → ITEM-028..032 (OmniRouting slices a-e). Parents archived as superseded (story/ACs/open decisions retained there). Slice dependency pull-order preserved: guard item chain before OmniRouting chain. ITEM-031/032 (evolution loop, PagedLoRA) remain staged/gated on the parked Slice 3 — in the list, not pullable.
- **Staged-item decisions (PO)**: unpark trigger for ITEM-031/032 (and Slice 3) is **backlog order itself** — no separate data trigger; when the items reach the top they're pulled, by definition unparked. GPU economics for ITEM-032 **err toward the most performant model** (cost secondary) — persistent best-in-class base is the default direction when the GPU tier activates. Recorded in ITEM-031/032 files.

## 2026-09-04 — Slice 1 complete; backlog replenished

- **Archived**: ITEM-001..007 (Slice 1: docker scaffold, schema, zero-copy proxy, tenant auth, telemetry, cost linter, dashboard) — all ACs verified live on disposable Vultr hosts; 45 tests green.
- **Refinement session**: backlog was empty (Rule 5 — flagged). Replenished from the documented gap shortlist + the roadmap's next slice.
- **New items (single-ranked, small→large)**: ITEM-008 DeepSeek real pricing (dashboard $ are wrong today), ITEM-009 traces on stream abort (data loss), ITEM-010 rule B counts survive restart, ITEM-011 runbook, ITEM-012 smoke coverage, ITEM-013 operator view (flagged: conflicts with ITEM-007 AC5 tenant isolation — needs an operator auth mechanism), ITEM-014 durable deployment (first always-on spend — business decision, break down when it rises), ITEM-015 + ITEM-016 Slice 2 (heuristic router, Postgres budget breaker — left chunky per progressive granularity).
- **Product goal updated** for the next horizon: durable api.inferon.dev + automatic cost protection.
- **Items deliberately not backlogged**: future Slices 3–5 (distillation/serving/autonomous) — too large and low-probability; will be captured when relevant.

## 2026-09-04 (later) — ITEM-008 archived; ITEM-017 captured

- ITEM-008 (DeepSeek real pricing) done + archived after live verification.
- **Captured ITEM-017** (cache-hit-aware costing) — flagged follow-up to ITEM-008: DeepSeek auto-caches long prefixes and bills cached input at ~$0.007 vs $0.22 miss, so the single miss-rate row overstates repeated-prompt spend up to ~30×. Ranked #3 (after the 009 trace-data-loss bug, before 010) because it affects every cached DeepSeek trace's reported cost. `cached_prompt_tokens` is already stored per trace (ITEM-005), so this is a `calculate_cost` signature + worker call-site change only.
