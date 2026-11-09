## ITEM-136: The runtime report measured the wrong thing

**As** the operator reading the Runtime page
**I want** it to judge the claim's own traffic and show where the time goes
**So that** a large prompt, a slow upload or a measurement change cannot look
like a proxy regression

**Reported** 2026-11-08 (PO), straight after ITEM-135: "the current runtime report
is very misleading". ITEM-135 made the gateway number honest; that exposed three
ways the report around it still was not.

**What was misleading**

1. **The verdict judged the wrong traffic.** `contract_verdict` took every request
   in the window, so a 285k-token prompt was measured against a claim written for
   "a warm request carrying a small prompt". The page then printed the payload size
   beside the verdict — admitting the mismatch instead of fixing the comparison.
   With real traffic at 285k tokens, "over the 1 ms claim" was inevitable and
   meant nothing.
2. **One blended percentile described neither end of the traffic.** A single p50
   averaged 4 KB and 1.2 MB requests. An operator could not tell a regression from
   a change in what people send, which is the whole question the page exists for.
3. **Two measurement eras were averaged together.** Rows written before
   `body_ms` existed counted the client's upload as gateway time; rows after do
   not. A window spanning both shows a step change that looks like a slowdown.

**Delivered**

- **The verdict reads the claim's basis only** — `prompt_tokens <= 4_000`
  (ITEM-136's `SMALL_PROMPT_TOKENS`, the size the claim was calibrated on), with
  `MIN_SAMPLES` still required. No small-prompt traffic → no verdict, and the page
  says "no prompts under 4,000 tokens in this window" rather than a red mark.
- **A payload-band table replaces the blended figure** as the primary view: small
  (≤4k), medium (≤50k), large (≤150k), huge (over 150k), each with requests,
  gateway p50/p95 and **the client's median upload beside it**. An empty band is
  hidden, never rendered as a healthy zero.
- **One era only.** Overhead, bands, the verdict and the per-model table all read
  rows with `body_ms IS NOT NULL`, so the report covers traffic measured by the
  current clock. `contract.basis.measured_since` records when that started, and
  `excluded_requests` counts what was left out — the page states both, because an
  empty report that does not explain itself reads as broken.
- The headline KPIs are "Gateway time, small prompts" and "Slowest 5%, small
  prompts", and the under-1 ms rate is scoped to the same basis (it was
  structurally 0% on 1.2 MB traffic and presented as a health metric).
- The per-model table gains the upload column and the same era filter — a
  per-model average across the two clocks reintroduces exactly the misreport the
  bands remove.

**Tests**

- `BANDS`/`band_of`/`band_bounds` are pure and covered: inclusive boundaries, no
  gap between bands, no overlap, an open-ended last band, and a trace with no token
  count is `unknown` rather than guessed into a band.
- 585 Rust tests, clippy, tsc, eslint, `i18n-check` clean.
- The SQL is verified live rather than unit tested (the dashboard queries need the
  real pool); the numbers below are that verification.

**Verified live** (rebuilt images, tenant with real traffic)

```
contract basis   {'max_tokens': 4000, 'measured_since': '2026-11-09T03:13:33Z'}
band  small   n=1   gateway p50 52,445 µs   upload p50 16 ms
```

and the rendered page reads:

```
Gateway time by prompt size | Prompt size | Requests | Gateway p50 | Gateway p95 | Your upload, p50
                            | Small · up to 4,000 tokens | 1 | 52,445 µs | 52,445 µs | 16 ms
```

The 52 ms is a single cold request (config load) and the verdict correctly reports
"no verdict, too few samples" instead of a pass or a fail.

**Not done**

- The report does not yet chart the bands over time; a band moving while its
  payload stays put is the shape of a real regression, and that wants a series, not
  a row. Worth an item if the question comes up again.
- `latency` (end-to-end) deliberately still spans both eras: `total_duration_ms`
  always included the upload and the provider, so it is comparable across the
  change. Stated here so nobody "fixes" it into incomparability later.

**Source:** PO report, 2026-11-08.
