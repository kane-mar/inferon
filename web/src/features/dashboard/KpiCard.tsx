// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import type { IconName } from './Icon';
import type { Tone } from '@/libs/reportTone';
import { trend } from '@/libs/dashboardMetrics';
import {
  KPI_CARD,
  KPI_SUBTLE,
  kpiDeltaClass,
  kpiValueClass,
  toneGlyph,
  toneGlyphClass,
} from '@/libs/reportTone';
import { cn } from '@/utils/Helpers';
import { Icon } from './Icon';

/**
 * The one metric card every report uses (ITEM-106): a value with its unit and
 * the window it covers, plus the definition on hover.
 *
 * `goodWhen` marks which direction is an improvement (spend/down, requests/up).
 * `tone` highlights a value that needs attention — the card is not tinted, only
 * the value and its glyph, so colour stays a highlight rather than decoration.
 *
 * ITEM-138: the card states the number and nothing else. A definition on a card is
 * prose on a screen: the label carries the scope (payload, window), the value and
 * unit carry the quantity, and anything that still needs a sentence is a
 * `DocsLink` in the section header instead.
 */
export const KpiCard = (props: {
  label: string;
  value: string;
  /** A line icon for this measure; decorative, so the label carries the meaning. */
  icon?: IconName;
  /** The unit, when the formatted value does not already carry it. */
  unit?: string;
  /** The days this value covers — stated on the card, never inferred. */
  window?: number;
  delta?: number | null;
  goodWhen?: 'up' | 'down';
  /** A verdict, not an explanation: "over the claim", "no traffic". */
  verdict?: string;
  tone?: Tone;
}) => {
  const delta = props.delta ?? null;
  const direction = trend(delta);
  const improved
    = direction === 'flat'
      ? null
      : ((props.goodWhen ?? 'up') === 'up') === (direction === 'up');

  return (
    <div className={KPI_CARD}>
      <div className="flex items-baseline justify-between gap-2">
        <div className={cn('flex items-center gap-1.5 text-sm font-medium', KPI_SUBTLE)}>
          {props.icon && (
            <Icon name={props.icon} size={16} className="text-muted-foreground" />
          )}
          {props.label}
        </div>
        {/* Locale-neutral: the page's window chip spells the period out. */}
        {props.window != null && (
          <span className={cn('text-xs tabular-nums', KPI_SUBTLE)}>
            {props.window}
            d
          </span>
        )}
      </div>
      <div className="mt-2 flex items-baseline gap-2">
        {toneGlyph(props.tone) && (
          <Icon
            name={toneGlyph(props.tone)!}
            size={16}
            className={toneGlyphClass(props.tone)}
          />
        )}
        <span className={kpiValueClass(props.tone)}>{props.value}</span>
        {props.unit && (
          <span className={cn('text-sm', KPI_SUBTLE)}>{props.unit}</span>
        )}
        {delta != null && direction !== 'flat' && (
          <span
            className={kpiDeltaClass(props.tone, improved === true)}
            title={props.goodWhen === 'down' ? 'lower is better' : 'higher is better'}
          >
            <span aria-hidden>{direction === 'up' ? '▲' : '▼'}</span>
            {` ${Math.abs(delta).toFixed(1)}%`}
          </span>
        )}
      </div>
      {/* One short line, and only when it says something about the value itself.
          ITEM-138: never a definition — that is what the docs link is for. */}
      {props.verdict && (
        <p className={cn('mt-2 text-xs', KPI_SUBTLE)}>{props.verdict}</p>
      )}
    </div>
  );
};
