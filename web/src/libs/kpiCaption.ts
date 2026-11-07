// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * The lines printed under a metric value.
 *
 * ITEM-125: a metric explains itself on the card — never in a hover title that a
 * touch device cannot show. ITEM-127 puts the payload context in the caller's
 * verdict line, so both lines have to survive: a verdict must never replace the
 * definition it was written to accompany.
 */
export const kpiCaption = (lines: { hint?: string; definition?: string }): string[] =>
  [lines.hint, lines.definition].filter((line): line is string => Boolean(line?.trim()));
