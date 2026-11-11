// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * ITEM-148: composition, as widths a reader can trust.
 *
 * A report that shows six percentages in six boxes makes the reader do the
 * summing. One bar whose segments are the counts answers "what were these
 * requests" at a glance. The arithmetic is pure so it can be tested: a bar whose
 * widths were computed inline in JSX is a bar nobody checked.
 */

/** The smallest share a segment is allowed to claim, so 1-in-1000 is visible. */
const MIN_VISIBLE_SHARE = 0.4;

export type Segment = { key: string; count: number; share: number };

/** `part / whole`, as a percentage; `null` when there is no whole. */
export const share = (part: number, whole: number): number | null =>
  whole > 0 ? (part / whole) * 100 : null;

/**
 * Counts → segments, in the caller's order, with zero counts dropped.
 *
 * Shares are computed from the total of the parts (not a separate total), so a
 * bar always fills its width even if the API's counts do not add up to the
 * request count. A part that is real but tiny is lifted to `MIN_VISIBLE_SHARE`
 * so the bar never shows a hairline that looks like a rendering artefact.
 */
export const split = (parts: { key: string; count: number }[]): Segment[] => {
  const present = parts.filter(part => part.count > 0);
  const total = present.reduce((sum, part) => sum + part.count, 0);

  if (total === 0) {
    return [];
  }

  return present.map(part => ({
    key: part.key,
    count: part.count,
    share: Math.max(MIN_VISIBLE_SHARE, (part.count / total) * 100),
  }));
};
