// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * ITEM-146: an empty report says so in one line, in the same place every time.
 *
 * Panels each invented their own muted paragraph, so an empty table, an empty
 * chart and an empty list looked like three different products. The dashed edge
 * also makes "nothing to show" distinguishable from "still loading" (which is a
 * skeleton) and from "a table with no rows" (which is a stray header row).
 */
export const Empty = (props: { label: string }) => (
  <p className="
    rounded-md border border-dashed px-4 py-6 text-center text-sm
    text-muted-foreground
  "
  >
    {props.label}
  </p>
);
