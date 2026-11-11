## ITEM-148: Rebuild the bill and quality layouts

**Reported** 2026-11-10: "the 'bill' and 'Quality' pages are really ugly. can you
improve the layout so that it is easier to understand."

**Why they were hard to read.** Both pages were stacks of equally weighted cards,
so nothing said what mattered, and both repeated their own numbers.

- **Bill** opened with five chart-plus-table cards, in which each bar chart printed
  exactly the same figures as the table beneath it. The six findings (the reason to
  care) sat after all of it. Nothing said what the total cost was before the
  detail arrived.
- **Quality** opened with six KPI boxes, five of them failure rates with warning
  icons, so the page read as six alarms. Then three full-width cards, two of them
  four rows tall. A reader had to add five percentages to learn what the traffic
  was.

**Bill now** answers what it cost before where it went: four numbers (spent,
requests, median per request, saved), then by model and by provider, then the
findings, then the long tails (by key, by prompt, transactions). The three
duplicate charts are gone: a **share bar** inside the cost cell shows the
proportion without a second copy of the same numbers.

**Quality now** carries four cards (success, error, truncated, requests) and one
**composition bar**: ok, empty, refused, truncated, error, unclassified, each with
its count and share, so the page states what the requests were instead of asking
the reader to sum. The two short lists (why requests failed, which prompt groups)
sit side by side rather than as full-width cards four rows tall, and every numeric
column is right-aligned and grouped.

**The arithmetic is pure and tested.** `libs/segments` (`split`, `share`) has 6
tests, including the 0.4% floor that keeps a 1-in-1000 segment visible and the rule
that zero counts drop rather than render as invisible slivers.

**Two defects the rebuild exposed**, both older than it:

1. `HEAD` carried `text-left`, so `HEAD_RIGHT` emitted both classes and the winner
   depended on stylesheet order: numeric column headers could sit left while their
   numbers sat right. `HEAD` is alignment-neutral now.
2. `SpendBreakdown` had its own `msOrDash` printing `820ms` (no space) and the
   runtime heatmap cell had an inline `${Math.round(...)}ms` — two private copies of
   the formatter ITEM-147 had just unified. Both use `duration` now, and the
   breakdown's `Provider` header (a bare `th`, which the browser centres) says
   `text-left`.

`BarChart` is deleted: nothing used it once the duplicate charts went.

**Verified live:** both pages 200, zero headers without alignment, zero private
`820ms` copies, six share bars on the bill, completion 200, 0 errors. 180 web tests,
tsc, eslint, knip, i18n-check clean.

**Not done:** no new charts. A bar that repeats a table's numbers is decoration; the
one bar that survived is the composition bar, which shows something the table cannot
(a shape at a glance).

**Source:** PO report, 2026-11-10.
