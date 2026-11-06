> **Delivered 2026-11-06.** Both grouped queries in `src/dashboard/datasets.rs` now
> drop rows that belong to no prompt (`system_prompt_hash IS NOT NULL`) and rows with
> no reason; live: `/v1/dashboard/api/datasets` returns `200` in both groupings and
> `/dashboard/datasets` renders. 553 tests, clippy clean.

## ITEM-126: The Datasets page returns 500 when one request had no system prompt

**As a** workspace owner opening **Datasets**
**I want** the report to load
**So that** the dataset loop (ITEM-086/087) is reachable at all

**Found 2026-11-06 during the UI review.** `/dashboard/datasets` answered `500` on the
live host (the page's only job is to render this report):

```
web-1  | ⨯ Error [InferonApiError]: the request could not be completed { status: 500, type: 'dashboard_error' }
proxy-1 | ERROR inferon::dashboard::respond: dashboard request failed:
          error occurred while decoding column 0: unexpected null; try decoding as an `Option`
```

**Cause.** `src/dashboard/datasets.rs` runs two grouped queries over `traces`. The
first filters `system_prompt_hash IS NOT NULL`; the **second** (the reasons query) did
not, and it selects the grouping key as a `String`. A request with no system prompt has
no prompt hash, so its excluded row groups under `NULL` and sqlx aborts the request —
one such row breaks the whole report for the tenant.

The row that exposed it on the live host: `no-such-model-123`, `example_reason=error`,
`system_prompt_hash=NULL` — a rejected request, which is exactly the traffic a dataset
report should simply leave out.

**Fix.** The reasons query takes the same `system_prompt_hash IS NOT NULL` filter as the
cluster query, plus `example_reason IS NOT NULL` (a null reason explains nothing), and
`request_model` is qualified. A row that belongs to no prompt belongs to no dataset.

**Acceptance Criteria**
1. `GET /v1/dashboard/api/datasets` returns `200` for a tenant that has excluded rows
   without a prompt hash, in both groupings (`hash`, `cluster`).
2. The count of excluded rows is unchanged for rows that do have a hash — the fix drops
   only the ungroupable ones.
3. Live: the Datasets page renders on the durable host, and the numbers match a direct
   SQL count.

**Source:** UI review, 2026-11-06.
