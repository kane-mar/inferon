// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import type { Segment } from '@/libs/segments';
import { share } from '@/libs/segments';

/**
 * ITEM-148: one bar instead of six boxes.
 *
 * The quality page showed `ok`, empty, refused, error and truncated as five
 * separate KPI cards, each with its own percentage. The reader had to add them up
 * to see what the traffic actually was. Here the segments are the counts, so the
 * proportions are the point and every segment prints its own number beneath.
 *
 * Colour is never the only signal: the legend carries the label, the count and the
 * share, and each swatch is paired with that text.
 */
const SWATCH: Record<string, string> = {
  ok: 'bg-success',
  empty: 'bg-muted-foreground/40',
  refused: 'bg-warning/60',
  truncated: 'bg-warning',
  error: 'bg-destructive',
  other: 'bg-muted-foreground/25',
};

export const OutcomeBar = (props: {
  segments: Segment[];
  labels: Record<string, string>;
}) => {
  const total = props.segments.reduce((sum, segment) => sum + segment.count, 0);

  if (props.segments.length === 0) {
    return null;
  }

  return (
    <div>
      <div className="flex h-2.5 w-full overflow-hidden rounded-full bg-muted">
        {props.segments.map(segment => (
          <span
            key={segment.key}
            className={SWATCH[segment.key] ?? SWATCH.other}
            style={{ width: `${segment.share}%` }}
            aria-hidden
          />
        ))}
      </div>
      <ul className="mt-3 flex flex-wrap gap-x-5 gap-y-2 text-sm">
        {props.segments.map(segment => (
          <li key={segment.key} className="flex items-center gap-2">
            <span
              aria-hidden
              className={`
                size-2.5 rounded-full
                ${SWATCH[segment.key] ?? SWATCH.other}
              `}
            />
            <span>{props.labels[segment.key] ?? segment.key}</span>
            <span className="text-muted-foreground tabular-nums">
              {segment.count.toLocaleString('en-US')}
              {` · ${(share(segment.count, total) ?? 0).toFixed(1)}%`}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
};

/**
 * A thin bar inside a table cell: the same idea at row scale, for a cost column
 * or an error count. It replaces the chart that used to sit above the table
 * repeating the same numbers.
 */
export const ShareBar = (props: { share: number | null; tone?: 'accent' | 'bad' }) => {
  if (props.share == null || props.share <= 0) {
    return null;
  }

  return (
    <span
      aria-hidden
      className="mt-1 block h-1 w-full overflow-hidden rounded-full bg-muted"
    >
      <span
        className={`
          block h-full rounded-full
          ${
    props.tone === 'bad' ? 'bg-destructive/60' : 'bg-chart-1'
    }
        `}
        style={{ width: `${Math.min(100, Math.max(2, props.share))}%` }}
      />
    </span>
  );
};
