## ITEM-137: A band drifting while its payload stays put

**As** the operator reading the Runtime page
**I want** each payload band over time
**So that** a real regression (same work, more time) looks different from bigger
prompts (more work, more time)

**Why** ITEM-136 left a snapshot: bands of small/medium/large/huge, each with one
percentile for the window. That answers "is the gateway slow *now*" but not the
question that follows — *is it getting slower?* A band whose p50 climbs could be a
regression or could be traffic that grew, and in a snapshot the two are the same
picture. This was recorded as the "not done" part of ITEM-136 and is the item that
closes it.

**Delivered**

- `latency::drift` classifies a bucket against its band's baseline: 1.25× is
  `watch`, 1.5× is `regression`, and **neither verdict is allowed when the payload
  moved more than 20%** — that bucket is `moved`, drawn muted and italic, never
  tinted. Missing token counts are incomparable for the same reason: guessing there
  is how a report lies.
- One query per band, `GROUP BY GROUPING SETS ((bucket), ())`, returns the
  per-bucket rows *and* the band total in a single round trip — so the baseline a
  bucket is judged against is literally the same measurement as the summary above
  it, not a second estimate that could disagree.
- The page draws a band × bucket grid: gateway p50 with the median prompt size
  beneath it in every cell, a glyph per verdict (▲ regression, △ watch, ~ moved),
  a legend, and a note stating the 20% rule. Colour is never the only signal.
- The grid appears only when a band has more than one bucket — one column is not a
  trend.

**The bug that writing it exposed**

The first version of the series printed the *band's* median payload in every cell,
so the number a reader compares ("did the prompt move?") was constant by
construction, and the `moved` verdict would have been unexplainable. Caught by
reading the API output against two fixture rows, not by the type checker — the
same lesson as ITEM-125: look at what the screen actually says.

**Tests**

- 4 Rust cases for the classification: a genuine drift, a payload move in both
  directions, the 20% boundary inclusive, and no verdict from missing numbers.
- 5 web cases for the presentation, including that a `moved` cell is never drawn
  with the destructive or warning ink.
- 589 Rust tests, clippy, tsc, eslint and `i18n-check` clean.

**Verified live** with two labelled fixture rows (one same-payload-slower, one
bigger-payload), then deleted:

```
band small   baseline 33,689 µs
  2026-11-07 00  90,000 µs  200 tok  => moved
  2026-11-08 00  60,000 µs   33 tok  => regression
  2026-11-09 00  15,498 µs   33 tok  => calm
```

and the rendered grid: `Small | 90,000 µs ~ 200 | 60,000 µs ▲ 33 | 15,498 µs 33`.

**Still not done**

- No line shapes: the grid is a table, not a chart. That is deliberate — a table
  prints its numbers, which the design language requires, and a sparkline would
  hide the payload that makes the verdict readable. If the window grows past a few
  dozen buckets the table needs a zoom or a cap; today the page buckets by day.
- The drift thresholds (1.25 / 1.5 / 20%) are constants, not per-band
  configuration. They are stated on the page and in `latency.rs`; if a workspace
  needs others, that is a settings change, not a redesign.

**Source:** continuation of ITEM-136 (PO: "okay, do the not done work").
