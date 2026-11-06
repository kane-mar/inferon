# Refinement Log

## 2026-11-07 — pullable region empty; what is left is a PO decision

Delivered this session, all live: the guide suite (ITEM-116..121, 29 pages), and the four
defects its evidence produced — ITEM-123 (a refusal now names its trace), ITEM-127 (gateway
time scales with the prompt, and the claim now carries its payload), ITEM-128 (the checker
covers buffered answers and a workspace's own cheap model), ITEM-129 (a keyless provider no
longer needs a `key` field). One regression of my own was found by reading the rendered
page rather than the diff: ITEM-125's card change made a definition replace a verdict line,
so the Runtime page lost the very context ITEM-127 added. Fixed, with the rule moved into a
pure function the unit suite covers.

What remains is not buildable by an agent:

- **ITEM-112** — the site builds (`mkdocs build --strict`, 29 pages), CI checks it and
  uploads the artifact; the host is a PO choice (Netlify Free recommended).
- **ITEM-029** — a re-decision, not a build: ITEM-089 supplies the input.
- **The goal's first clause** — an external developer, which is a handover, not an item.
- The gated block (076/032/030/072/084/114) is unchanged and unparkable on evidence.

## 2026-11-06 (late) — the guide suite is complete; evidence produced three defects

The backlog's pullable region was the guide suite. It is delivered: 29 published pages
(index, two getting-started, twelve configuration, thirteen troubleshooting, self-hosting),
each written from the code and verified against the live gateway, each carrying its five
sections where it is a troubleshooting page and a dated footer everywhere. `mkdocs.yml`
carries the full nav so a page cannot ship without appearing on the site.

Writing guides from the system rather than from memory is what made this worth doing: three
real defects came out of it, none of which the backlog had.

- **ITEM-127 (gateway time).** The Runtime page said 1.2 s while the product said
  sub-millisecond. The number is proportional to the payload; three whole-payload passes
  before dispatch were waste (a clone + re-serialisation, a schema serialised to be
  measured, a prompt serialised to be counted). Fixed, and the claim now carries its
  payload size everywhere. 1.2 MB went from 1.2 s to 23.8 ms p50 live.
- **ITEM-128 (the checker).** The output checker verified only *streamed* answers, and
  "cheap tier" was a hard-coded list of four model families — so the most common structured
  request was unchecked and a workspace's own cheap model never was. Both fixed and
  verified live, including the replay escalating instead of retrying itself.
- **ITEM-129.** Creating a keyless `local` provider over the API failed without a `key`
  field, with a raw serde error. One line plus two tests; delivered and verified live.

Ordering decision: the defects outranked the remaining guide pages once found, because a
guide that documents a limitation is a workaround and the fix is cheaper to read than the
paragraph. That is the rule this session followed: **write the page, and when it needs a
caveat, fix the caveat first.**

## 2026-11-06 — UI review: legibility, self-explaining screens, and a page that was 500

PO: "the fonts are small, and often hard to read"; "if the user needs the documentation
then there is a clear design failure". Measured before changing anything, which is what
made the fix small: 287 uses of 12/14px type, 49 `font-mono text-xs` label sites (metric
captions and table headers at 12px, uppercase, wide tracking), muted ink at 4.98:1 and
destructive at 4.27:1.

Decision: raise the scale at the design tokens (`--text-xs/sm/base` in `global.css`),
not at 287 call sites; convert labels to sans/sentence case; reserve mono for ids,
numbers and code. Contrast measured per token, not eyeballed: muted 7.2:1 light and
9.4:1 dark, destructive 5.7:1 and 7.3:1.

Second finding: the pages printed our vocabulary (`upstream_429`, `fallback_failed`) and
hid the meaning in `title` attributes, which is precisely why the guides had to teach
those words. One mapping (`web/src/libs/labels.ts`) + two locale namespaces fixed all
three surfaces, with the raw code kept as a secondary hint for support replies. A test
now fails if the backend gains a class with no label in either locale — the guard that
keeps the guides from becoming required reading again.

Third finding, and the reason this was worth doing in one pass: `/dashboard/datasets`
answered `500` on the live host. The reasons query grouped excluded traces by prompt
hash without the `IS NOT NULL` filter the cluster query above it has, so any workspace
with one prompt-less request (a rejected call, for instance) lost the whole dataset
report — the only door to the dataset loop. Fixed as ITEM-126, verified `200` in both
groupings.

## 2026-11-06 — ITEM-123 delivered; refusals are evidence

Fixed the two gaps ITEM-117 found: route resolution is now traced (`inferon_4xx`) and
every generated refusal carries the id of its row. Verified live on the rebuilt proxy —
`no_provider` (both surfaces), `502 upstream_error` (class `transport`), and our own `429`
(class `rate_limited`) each return a header that matches the row, and Quality counts the
`no_provider` attempt it previously could not see.

Method note: the 429 path cannot be unit-tested without a live database (the limiter
charges a Postgres window and fails open when it is unreachable), so it was verified on
the host while the other three paths got unit tests that assert the header equals the
traced id. A test that asserts *the id the caller holds is the row that was written* is
cheap and catches the exact bug this item was.

## 2026-11-05 (later still) — the docs site is built; the host waits

PO: "build first, choose later." Done: `mkdocs.yml` (`docs_dir: guides`, no page moves,
`exclude_docs` for the two sales documents), `requirements-docs.txt` pinning
`mkdocs-material==9.8.0`, and a CI `docs-site` job that installs the pin, builds with
`--strict`, asserts no sales document was published and uploads `site/` as an artifact.

Verified by building the real tree in `python:3.12-slim` on the durable host rather than
trusting a laptop Python (this machine is 3.8; Material 9.8 needs 3.11+): 8 pages, 404
page, 40 KB search index, `.md` cross-links rewritten to directory URLs, section anchors
present, strict clean. MkDocs rewrites the `.md` links, so the repository files stay
GitHub-readable and the site works without a second copy.

Consequence for the backlog: ITEM-112 no longer has build work. It is a one-line host
decision plus DNS. Flagged for the future: Material EOL 2027-05-05, successor Zensical
reads the same config.

## 2026-11-05 (later) — ITEM-117 delivered; two data-plane gaps captured as ITEM-123

Four troubleshooting pages went live (401/403, no_provider, 429 ours-vs-provider,
5xx/exhausted chain). Writing them against the live host produced evidence the items did
not anticipate, and two of those findings are defects, not prose:

- **`no_provider` writes no trace.** A workspace with no provider answers `400
  no_provider`, and three seconds later has zero rows in `traces`; Quality reads
  `requests: 0`. The most likely first support question leaves no evidence.
- **Gateway-generated errors carry no `x-inferon-trace-id`.** `400 no_provider`, `429
  rate_limited` and `502 upstream_error` come back without it, while a relayed provider
  error does. The rate-limited and transport rows exist, so the gap is the header, not
  the record.

Captured as **ITEM-123** and ranked first: small code, one proxy deploy, and it removes
the caveat the new pages had to write ("this failure leaves no trace"). Deliberate
method note: the pages state the gap plainly rather than hiding it; the item closes it.

Working method that made this cheap — a mock OpenAI-compatible upstream (three models:
`-ok`, `-429`, `-500`) as a throwaway container on `inferon_default`, plus a temporary
workspace, owner and member users, a minted session and per-key limits. Every scenario
(our 429, relayed 429/500, chain step, exhausted chain, transport 502, member 403,
revoked 401) was reproduced end to end and torn down afterwards.

## 2026-11-05 (later) — Cloudflare removed from ITEM-112; host choice reopened

PO instruction: remove Cloudflare from the docs-site item and find something else,
confirming with the PO before committing. The candidates and the constraint that
decides them are recorded in the item; the recommendation is **Netlify Free** (the
only $0 host that accepts the private repository, allows commercial use and gives PR
previews), with **GitHub Pages** second (best once the repository goes public, which
the AGPL positioning argues for) and **Read the Docs Community** only once it is
public (their free tier is for open-source projects). The generator decision —
MkDocs Material with `docs_dir: guides` — is unaffected, so the build can be written
and previewed while the host is pending.

## 2026-11-05 — ITEM-113 broken into five slices; the first is delivered

The pullable region held one item (ITEM-112, blocked on a PO-owned Cloudflare
project) plus ITEM-113, which is 26 pages and too large to pull whole. It was split
by audience task, and the slice boundary is where verification stops being one
afternoon's work:

- **ITEM-116 — skeleton + getting started** (index, first request, move an SDK).
  Delivered the same day. It also fixes the suite's shape: `guides/README.md` as the
  entry point, a dated `Last verified` footer on every page, and
  `scripts/check-guides.py` + a CI `docs` job that fail on a broken link, a link that
  leaves `guides/` or points at a sales document, a missing footer, a troubleshooting
  page without its five sections in order, a mean sentence over 25 words or any
  sentence over 40.
- **ITEM-117 — troubleshooting, access and routing** (401/403, no_provider, 429,
  5xx chain). Ranked above the configuration slices: with no support channel, the
  failed request is what costs a developer the product.
- **ITEM-118 — what answers your requests** (providers incl. a local model, models
  and tier pairs, output checker, automatic router).
- **ITEM-119 — the bill and the context** (budgets and limits, fallback chain and
  triggers, prompt caching, pruning and the comparison group).
- **ITEM-120 — traffic to data and the money story** (payloads/retention/PII,
  datasets → gold sets → evals, the cost audit's findings, sessions and spend).
- **ITEM-121 — troubleshooting, operations** (truncated answers, quality dropped,
  unreachable provider, empty dashboard, `$0.00` audit, sign-in loop, 502,
  migrations, purged payloads).

Ordering decision: ITEM-112 keeps its place above the slices and is marked **blocked
on PO** rather than gated, because nothing about it is uncertain — it needs a
Cloudflare project and a DNS record that only the PO can create. The guides publish
from `guides/`, so the slices are useful before the site exists.

Captured during the live verification of ITEM-116: **ITEM-122**, the Providers empty
state tells a self-serve workspace its traffic uses the shared platform key, which is
false (`400 no_provider` on the first request). Ranked first: it is a one-key copy
change, and it costs a new user their first five minutes.


## 2026-09-06 (late) — ITEM-033 broken down; ITEM-018 tail structured

Refinement session with the pullable region empty (guard chain delivered,
OmniRouting staged). ITEM-033 (user accounts & logins) was the next large item
— broken into seven refined, ranked pullable slices; parent archived as
superseded (story/PO answers retained):

- **ITEM-034 — API keys as rows**: `api_keys` table + backfill from
  `tenants.api_key_hash`; auth resolves active keys → tenant (existing keys
  keep working); operator mint/list/revoke API; foundation for all key work.
- **ITEM-035 — GitHub OAuth + sessions**: /auth/login+callback, `users` rows,
  signed session cookie, middleware; mock-GitHub testable; `/v1/chat` stays
  key-authed (design principle recorded: humans get sessions for dashboards,
  machines keep keys — the ITEM-004 hot path is untouched).
- **ITEM-036 — Self-serve signup**: first sign-in auto-provisions tenant +
  owner (`tenant_members`) + first key shown once; session-scoped tenant
  dashboard added alongside key auth; README primary path flips to self-serve.
- **ITEM-037 — Operator console**: platform operator role (bootstrap default:
  env `OPERATOR_GITHUB_LOGINS`; decision recorded), the 4 operator JSON APIs
  accept a session while `OPERATOR_API_KEY` survives for machines, first HTML
  operator screen.
- **ITEM-038 — Tenant key management (dashboard)**: create/label/revoke;
  owners destructive, members view+create (decision recorded).
- **ITEM-039 — Tenant members**: GitHub-claim-based invites (no email infra —
  decision recorded); member scope + removal; isolation extended to sessions.
- **ITEM-040 — Migrate out-of-band onboarding**: README/docs flip to
  self-serve, seed-tenant.sh demoted to operator/machine, ITEM-018 handoff
  path closes (staged key stays valid as an ITEM-034 key row).

Ranking: the accounts horizon sits directly below ITEM-018 and above the
staged OmniRouting cluster — the product goal's demand side (real external
developers) precedes the autonomy roadmap; signup (036) precedes the operator
console (037) for the same reason.

**ITEM-018** stays whole at the top (verified; its residual is one PO-gated
human handoff). Its tail is decomposed in the item file: [PO: name the
developer + hand over the staged bundle] → [pull: run first request + capture
AC5 findings]. Ordering decision for the PO: hand out the staged key now (the
fastest real-traffic data point, feeds ITEM-036's signup UX) vs have that
developer self-serve once ITEM-036 ships. Either way ITEM-040 later closes the
out-of-band path for everyone.

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

## 2026-10-04 — ITEM-028 unparked for build (Option 1) + durable host lost

- **ITEM-028 decision (PO):** Option 1 — re-baseline AC2 provisionally (≤10 ms on
  the production tier) and accept a linux-gated build loop; sequencing
  **traffic → measure on host → finalize AC/model**. Feature-gated ORT landed
  (`embedder` cargo feature; `load-dynamic` keeps the macOS loop untouched and
  even type-checks the ORT code; `NoEmbedder` fallback preserves AC4). Host
  measurement pending infrastructure.
- **Production incident captured as ITEM-041 (top of backlog):** the Vultr
  account holds **0 instances** — the durable host was deleted (account healthy
  and paid; not a billing suspension; no teardown was run by the agent). The
  public endpoint is down; DNS still points at the dead IP. Snapshots survive,
  newest inferon one 2026-09-05 (`4dca04b7`, pre-014e: 1 tenant, 2 traces) →
  data since 2026-09-05 is lost. PO decision needed: restore-from-snapshot (R1)
  vs rebuild fresh (R2) vs stay down (R3) — ~$6/mo either way.
- Blocks: ITEM-028 host measurement and all live verification.

## 2026-10-06 — control-plane horizon captured (ITEM-043..048)

PO review: the data plane is well ahead of the tenant-facing configuration. A
tenant cannot bring their own provider, choose models, set fallback rules, or
configure routing/budget from the UI, and the onboarding flow does not back the
"point your SDK at it" claim. Captured six items, ranked above ITEM-018:

- **ITEM-043 Provider provisioning** — tenant adds encrypted provider creds
  (deepseek/openai/anthropic/openai-compatible); the proxy resolves the tenant's
  provider; supersedes the shared platform key. Foundational.
- **ITEM-044 Model catalog & selection** — per-tenant enable/disable + default,
  metadata sharing one source with the cost engine.
- **ITEM-045 Fallback & routing rules** — ordered chain with triggers
  (error/timeout/429/guard); generalizes the guard's frontier replay.
- **ITEM-047 Dashboard spend breakdowns & limits** — by provider/model/key/prompt
  + per-key caps; reuses the 2026-10-05 chart primitives.
- **ITEM-048 SDK onboarding flow** — connect page (base URL + key + snippets)
  that detects the first trace.
- **ITEM-046 Routing policy & budget controls** — UI for ITEM-015/016.

Ranking rationale: 043 → 044 → 045 are dependency-ordered and are the minimum for
a tenant to run their own traffic; 047/048 make it usable; 046 is power-user
configuration. ITEM-018 (the PO handoff) now sits after the control plane.

## 2026-10-06 — ITEM-049 captured (own the app shell)

Follow-up to the design pass. The web app's *content* is Inferon-written, but its
*frame* is still the vendored SaaS-Boilerplate shell (the `(auth)` route group,
`dashboard/layout.tsx`, `DashboardHeader`, `MobileNavigation`, `TitleBar`,
`SlashIcon`, `CenteredContainer`, and template flourishes like the `/` separator
and shadowed header). And there are two operator consoles: the Rust
`/v1/dashboard/operator` HTML (bearer/operator-key, system fonts) and the Next
`/dashboard/operator` (session, themed) — the README points at the Rust one.

Captured as **ITEM-049** (type: tech-debt), ranked after the control-plane items
(043–048) since it is consistency housekeeping, not new capability. One item
covers both because they're the same "own all the surfaces" cleanup: exit the
template, and leave one operator console (JSON APIs for machines unchanged).

## 2026-10-28 — Connectivity decision: three profiles, ITEM-080 dropped, 081–084 captured

PO review of customer-hosted connectivity. The framing question was whether
Inferon routes cloud traffic to an on-prem model, and the answer is that it
cannot without the prompt transiting the cloud — so the option space is three
profiles, not two, and one proposed item was pointing the wrong way.

**The three recognised profiles**
1. **Self-hosted** — the whole stack on the customer's Docker (same image, same
   compose). Firewall mode. Already works; needs distribution polish (ITEM-083).
2. **Cloud + reachable endpoint** — an on-prem model published behind TLS and
   registered as a provider. Already built: ITEM-051 (pre-add probe, persisted
   reachability, discovery) + ITEM-077 (`local` kind priced $0 with the
   counterfactual still attributed). Needs the five-minute flow (ITEM-082) and a
   router axis that can point at it (ITEM-081).
3. **Edge node** — clients talk to it, prompts never leave the network. Keep for
   privacy customers; it is *not* a routing target for cloud traffic.

**Dropped: ITEM-080 (edge hybrid fallback).** A local-first-then-remote fallback
on the node reproduces exactly the failure mode it was meant to fix — "local
until it silently isn't" — and breaks the privacy promise that is the only
reason to run an edge node. If a tenant wants cloud models, they should be a
cloud tenant with a reachable endpoint (profile 2), not an edge node with a
backdoor.

**Captured**
- **ITEM-081 per-tenant tier pairs** — the actual "cheaper option": the tenant
  names the cheap model (including its own `local` one) instead of living with
  the hardcoded `TIER_PAIRS` const.
- **ITEM-082 attach an on-prem model as a provider** — keyless `local`, a real
  badge/hint, periodic reachability so a dark box is visible before a request
  fails.
- **ITEM-083 self-hosted distribution** — compose profile, install/upgrade/
  air-gapped guide, CI smoke of the documented path, plain support boundary.
  Mostly docs; no licence enforcement, no phone-home.
- **ITEM-084 connector / reverse tunnel** — **staged**. Unpark only if a
  customer refuses both a reachable endpoint and Docker.

**Centre of gravity: cloud.** Evidence: there is no licence, no billing, no
support or upgrade channel (so self-hosting as the *product* would mean building
those instead of product); every delivered differentiator (cost audit, router,
budget guard, savings attribution, adaptive policy) is cross-provider and
cloud-side; and the only real traffic evidence (ITEM-018) is cloud-side. So
ITEM-081 → 082 → 083. If that ever inverts, ITEM-083 moves to the top and the
other two do not change.

## 2026-10-30 — Competitive positioning brief: 16 gaps captured (ITEM-085..100)

PO supplied a positioning brief: the defensible edge is not routing or cost charts
(commodities) but closing the loop between production traffic and private assets —
a sub-millisecond proxy that lints runaway prompt tokens in flight and turns
verified traffic into deduplicated JSONL by task signature, then routes to a
distilled adapter. Source saved verbatim in
`raw/llm-inference/2026-10-30-competitive-positioning.md`; the code-grounded
comparison (claim vs. measured state, per row) is
`wiki/llm-inference/inferon-differentiation-gaps.md`.

**This changes the product goal**: distillation was "deliberately parked last"; it
is now the claimed edge and the top of the backlog. The earlier roadmap items did
not change rank relative to each other — they are still gated — but the new work
sits above them because it is the differentiator.

Measured before ranking (live, 7 days, 775 traces / 2 tenants, one ~800k-token
workload): prompts stored 774/774, completions **12/774** (streams store none);
guard fired 0; `proxy_ms` p50 **12 ms** / p95 **35 ms** for small requests, 1.9–2.5 s
for 800k-token bodies; ~10 store reads per request; the ONNX embedder has no
consumer.

Captured (rank order, with the claim each closes):

1. **085** persist streamed completions — the flywheel's missing leg (blocker).
2. **086** verified-example curation + per-prompt counts — makes "2,000 verified
   examples" a fact.
3. **087** dataset surface + JSONL export — the demonstrable moment.
4. **088** retention & consent — the gate that makes 087 shippable.
5. **092** hot-path config cache + measured latency budget — makes the runtime
   claim honest (or withdraws it).
6. **089** semantic task signatures — wire the unused embedder.
7. **090** in-flight payload lint (advisory) — the pre-flight half.
8. **093** conversation/transaction grouping — the economic unit for agentic loops.
9. **091** opt-in context pruning — high value, high risk, after 090's evidence.
10–16. commodity parity: **094** cross-format translation, **095** rate limits,
   **096** PII, **097** prompt registry, **098** eval harness, **099** heatmap,
   **100** load balancing.

Deliberately **not** duplicated: "route to a distilled adapter" is ITEM-032 and
"real trainer + gold set" is ITEM-076; the new items reference them.

## 2026-10-30 — Re-rank: one list, ordered by value then effort, gated at the bottom

The active set was previously ordered by narrative pillar, and the gated items sat
between active and delivered work. PO asked for a single list ordered by value and
effort, with blocked work at the bottom.

Scored each item value (1–5) and effort (1–5); dependencies override the score
where an item cannot usefully start before another. Final order and the reason:

1. **085** V5/E1 — unblocks every distillation item.
2. **086** V5/E2 — makes captured traffic a countable asset; 087 needs it.
3. **088** V4/E2 — cheap consent/retention gate; must precede 087's exposure.
4. **087** V5/E2.5 — the demonstrable moment (needs 086 + 088).
5. **092** V4/E2.5 — makes the latency claim honest; 090 needs its prompt cache.
6. **093** V3/E1.5 — cheapest remaining item; names the transaction.
7. **090** V3.5/E2.5 — the pre-flight linting half.
8. **089** V3.5/E3 — grouping quality; does not block 087.
9. **091** V4/E4 — highest value/highest risk (changes answers); after 090's evidence.
10. **094** V4/E4 — adoption unlock, but commodity parity.
11. **095** V3/E2.5 — rate limits (parity).
12. **096** V3/E2.5 — PII (parity; value rises with 087).
13. **098** V3/E3 — eval gate; its consumer (076) is infra-gated.
14. **100** V2/E2 — load balancing (health weights already cover failure).
15. **097** V2/E2 — prompt tooling (parity).
16. **099** V1.5/E1 — heatmap: cheap, lowest value.

Gated, at the bottom of the same list, ordered by the value they would add when
unblocked: **076** (training/gold set) → **032** (adapter serving) → **029**
(centroids) → **030** (solver) → **072** (cluster preference) → **084** (connector).
029 must precede 030/072.

Every item's `**Ranking note.**` now carries its rank and V/E score, so the order is
auditable from the item file alone.

## 2026-11-05 — public face: licence, hosted docs, guide suite; the for-profit path staged

PO request, not a delivery defect: the project needs a public face for traction
(hosted documentation, task guides) and a licence that stays honest as open source
now but does not block a company later selling the hosted service **and** a licence
of the Docker image.

Ranked the three pullable items value-first, effort as the tie-breaker:

1. **111 — licence, CLA, trademark** (V5/E1). The only item that blocks everything
   else: with no root `LICENSE` the repository is "all rights reserved", so today
   nobody may legally deploy it and the open-source publicity plan is empty. One
   afternoon of work, and it changes what every later publication may say.
2. **112 — hosted documentation site** (V4/E2). Publishing the existing README and
   guides is worth more than any new page: it turns the repo into a link you can send
   someone. Cloudflare Pages because its free plan has no bandwidth ceiling, a custom
   domain with TLS, and per-pull-request previews; Vercel's free tier is excluded
   because its terms restrict commercial use. docs.rs covers the Rust API for free.
3. **113 — guide suite** (V4/E3). The content that makes 112 worth visiting, and the
   answer to the support questions we already see (`no_provider`, exhausted chains,
   "quality dropped", empty dashboard). Larger than 112, so it sits below it.

**Licence decision, recorded so it is not quietly reversed.** AGPL-3.0-only for the
core, with a CLA so the owner can also license commercially. MIT/Apache-2.0 gives the
best adoption but hands a competitor the right to host a closed fork and leaves
nothing to sell; BSL/FSL protect the business but cannot be called open source, which
kills the stated traction mechanism; open core keeps the core adoptable but only
monetises the add-ons. AGPL is the only option that keeps the open-source claim, makes
a hosted competitor publish their changes, and creates the thing a commercial licence
sells — relief from copyleft plus warranty and indemnity. Cost accepted: AGPL is on
many corporate deny-lists, which is friction for exactly the developer audience we
want; the recorded fallback is Apache-2.0 + proprietary enterprise modules if that
friction turns out to be the bigger obstacle.

**Gated, not pullable:** *114 — commercial licence and paid image*. Selling a licence
to a customer who does not exist yet is waste. Unpark on the first "we cannot use
AGPL", or 10 active external workspaces, or procurement stopping on the licence. Its
placement (bottom of the gated block, after 084) is recorded in
`scripts/check-backlog-shape.py`'s `GATED_ORDER`.

The product goal's "first clause" — putting this in front of real external
developers — now has work behind it for the first time: 111 makes the code usable,
112 makes it findable, 113 makes it adoptable.

**Captured during implementation (2026-11-05): ITEM-115, ranked first.** The
ITEM-111 licence audit ran `npm audit --omit=dev` and found two live production
vulnerabilities: `next` 16.3.8 (critical) and `zod` 4.6.5 (high), both patch-fixable.
A defect in shipped code outranks feature work, so it sits above ITEM-112; the
public-site items shuffle down one place without any change to their value.
