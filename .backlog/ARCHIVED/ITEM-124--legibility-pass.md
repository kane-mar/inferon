> **Delivered 2026-11-06.** Type scale raised at the tokens: `text-xs` 13px, `text-sm`
> 15px, body 17px/1.65. Labels and table headers are sans, sentence case, medium
> weight; mono is reserved for ids, numbers, code and hashes; table rows gained
> `py-2.5`. Muted ink 7.2:1 light / 9.4:1 dark (was 4.98:1), destructive 5.7:1 / 7.3:1
> (was 4.27:1). Verified in the CSS the host serves: only 13/15/17px sizes are
> emitted. `/fr` and the marketing pages follow the same scale.

## ITEM-124: Legibility pass — type scale, contrast and label typography

**As a** workspace owner reading the dashboard for the first time
**I want** text I can read without leaning in, zooming or switching to dark mode
**So that** I can tell what the numbers mean in one pass

**Context (PO, 2026-11-06).** "The fonts are small, and often hard to read. Make the
website more legible." Measured in the code before changing anything:

- **287 uses of 12px and 14px type** (`text-xs` ×110, `text-sm` ×177) against a 16px
  base that almost nothing uses. Two places go below 12px (`text-[10px]`, `text-[11px]`).
- **Labels are the worst case for reading:** 49 `font-mono text-xs` sites, of which the
  metric labels and table headers also add `uppercase` + `tracking-wide` — 12px, mono,
  all caps, letter-spaced, on the muted ink.
- **Muted ink is only just passing:** `#6f6f6f` on `#fefefe` is 4.98:1 (AA needs 4.5 for
  body text, and 12px is not body text). `destructive` `#d6455f` is 4.27:1, under AA for
  small text. The pink accent `#f386a1` is 2.38:1 — fine as a fill, unusable as text.
- Rows are `px-3 py-2` at these sizes, so density is high and the line length long.

**Acceptance Criteria**
1. The type scale is raised at the tokens, not page by page: `text-xs` 13px/1.25rem,
   `text-sm` 15px/1.5rem, `text-base` 17px with generous line height. No text renders
   below 12px anywhere in the app.
2. Labels, table headers, chips and KPI captions use the sans face at 13–15px, medium
   weight, sentence case, normal tracking. Mono is reserved for identifiers, numbers,
   code and hashes — never for a label at small size.
3. Muted ink reaches ≥ 6:1 on both canvases; `destructive` reaches ≥ 4.5:1 for small
   text in both themes; the accent stays a fill, never text.
4. Table cells gain vertical room (`py-2.5`) and numbers stay tabular so columns still
   align.
5. The landing page's nav, footer and eyebrow follow the same scale, so the marketing
   surface and the product read as one product.
6. Verified: the built CSS carries the new tokens and no sub-12px class remains; the
   dashboard pages render 200 on the live host after deploy.

**Technical Notes**
- Tailwind v4 keeps the scale in `@theme` (`--text-sm`, `--text-sm--line-height`), so
  this is one file (`src/styles/global.css`) plus the shared atoms
  (`reportFormat.ts`, `KpiCard.tsx`) rather than 287 edits.
- No layout restructuring: same grid, same data, only size, weight, case and contrast.

**Source:** PO request, 2026-11-06.
