// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * ITEM-137: how a band's bucket is drawn.
 *
 * The classification is decided server-side (`latency::drift`) because it needs
 * the band's baseline and the payload comparison; this module only maps it to
 * presentation. The rule the colours encode: a cell may only be tinted as a
 * regression when the payload did not move. A bigger prompt is not a slower
 * proxy, and a chart that says otherwise sends somebody hunting a bug in the
 * gateway that is really a change in what people send.
 */

export type Drift = 'calm' | 'watch' | 'regression' | 'moved' | null;

/** The cell's surface. Only a comparable, slower bucket takes colour. */
export const driftCellClass = (drift: Drift): string => {
  switch (drift) {
    case 'regression':
      return 'bg-destructive/15 text-destructive';
    case 'watch':
      return 'bg-warning/10 text-warning';
    case 'moved':
      // Comparable? No. Marked, but never alarming: the work changed.
      return 'text-muted-foreground italic';
    default:
      return '';
  }
};

/** The glyph that repeats the verdict for a reader who cannot see the tint. */
export const driftMark = (drift: Drift): string => {
  switch (drift) {
    case 'regression':
      return '▲';
    case 'watch':
      return '△';
    case 'moved':
      return '~';
    default:
      return '';
  }
};

/** What the mark means, for the legend and for a screen reader. */
export const driftMeaning = (drift: Drift): 'regression' | 'watch' | 'moved' | 'calm' =>
  drift ?? 'calm';
