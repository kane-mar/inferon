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
import { kpiCaption } from '@/libs/kpiCaption';
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
 * ITEM-124/125: the label is sans and sentence case at 15px, and the definition is
 * printed on the card. It used to live in a `title` attribute, which a touch device
 * never shows and nobody hovers to discover — the page has to explain itself.
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
  /** What the metric means; falls back to the hint. Shown on hover. */
  definition?: string;
  delta?: number | null;
  goodWhen?: 'up' | 'down';
  hint?: string;
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
      {/* ITEM-125/127: the verdict and the definition are both printed — the
          caller's line first, the explanation under it. */}
      {kpiCaption(props).map((line, index) => (
        <p
          key={line}
          className={cn(
            'text-xs leading-relaxed',
            index === 0 ? 'mt-2' : 'mt-1',
            index > 0 && KPI_SUBTLE,
          )}
        >
          {line}
        </p>
      ))}
    </div>
  );
};
