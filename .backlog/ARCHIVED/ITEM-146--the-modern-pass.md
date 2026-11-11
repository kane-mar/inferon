## ITEM-146: The modern pass

**Requested** 2026-11-10: "review the ui and see what can be done to make it more
modern."

**What the review actually found.** Not taste: five things the code could prove
were wrong, each of which a modern dashboard is expected to get right.

1. **Nothing on screen while data loads.** No `loading.tsx` existed anywhere, so
   every dashboard route blocked on its queries with a blank screen. A slow report
   was indistinguishable from a click that did nothing. The segment now has a
   skeleton (header, three cards, rows) that keeps the shape of the page while the
   data arrives. It compiles into the app; with fast queries it never shows, which
   is the point.
2. **Spacing had no rhythm.** `mt-3`, `mt-4`, `mt-6` and `mt-8` did the same job on
   different pages, cards were padded `p-4` inside headers of `px-4 py-3`. Now: 6
   between sections, 5 inside a card, 4 inside a grid.
3. **The card contradicted the design language.** Every card carried `shadow-sm`
   while ITEM-134's rule is elevation by hairline. The shadow is gone, and the card
   clips its children so a wide table cannot break the corner radius. One
   `shadow-sm` remains, on the active nav tab, where a selected affordance wants
   the lift.
4. **Metric values read as text.** `text-2xl` with default tracking and no leading
   control. They are now 1.75rem, tight, and leading-free, so a KPI lands as a
   number.
5. **Twelve tables scrolled their header away**, in spend, datasets, operator and
   runtime. Sticky now.

**Also delivered**

- One `Empty` shape: panels each invented a muted paragraph, so an empty table, an
  empty chart and an empty list looked like three products. A dashed, quiet box also
  distinguishes "nothing to show" from "still loading" (a skeleton).
- A global `prefers-reduced-motion` opt-out, instead of one motion-reduce class on
  one dot.
- A keyboard focus ring on everything, not only where a component remembered one.
  The accent ink is 7.7:1 on the canvas.
- `scroll-margin-top` on anchors so a heading does not land under the bar, tabular
  figures by default in tables, and balanced headings.
- Two dynamic class concatenations the linter flagged became `cn()` calls, so a
  variant cannot be purged.
- Editing a dashboard page with the local `node_modules` symlinked exposed a stale
  `.next/types` and an incomplete npm install (a truncated cache entry). Both
  cleared; eslint, tsc, 169 unit tests and i18n-check are clean.

**Verified live:** five pages 200, six sticky headers on the bill page, skeleton
chunk present in the build, `prefers-reduced-motion` and `focus-visible` rules in
the served CSS, completion 200, 0 errors.

**Not done, and why**

- **No command palette (⌘K).** The modern expectation for a dashboard with this many
  surfaces, and it is a real feature rather than a polish pass: it needs a route
  index, keyboard handling, and a decision about what "search" means here (pages?
  workspaces? models?). Worth an item of its own.
- **No charts upgrade.** The bar list and the heatmap print their numbers, which is
  the design language; a sparkline would hide the payload that makes a runtime
  number readable (ITEM-137).
- **No density toggle or theme switch.** The interface has one density and follows
  the OS theme; both are preferences looking for a user who asked.

**Source:** PO request, 2026-11-10.
