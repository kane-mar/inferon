import type { IconName } from './Icon';
import type { Tone } from '@/libs/reportTone';
import { trend } from '@/libs/dashboardMetrics';
import { kpiCardClass, kpiDeltaClass, kpiSubtleClass, kpiValueClass, toneGlyph } from '@/libs/reportTone';
import { cn } from '@/utils/Helpers';
import { Icon } from './Icon';

const SEVERE = 'text-destructive';
const WARNING = 'text-amber-700 dark:text-amber-400';

/**
 * The one metric card every report uses (ITEM-106): a value with its unit and
 * the window it covers, plus the definition on hover.
 *
 * `goodWhen` marks which direction is an improvement (spend/down, requests/up).
 * `tone` highlights a value that needs attention — the card is not tinted, only
 * the value and its glyph, so colour stays a highlight rather than decoration.
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
    <div className={kpiCardClass(props.tone)} title={props.definition ?? props.hint}>
      <div className="flex items-baseline justify-between gap-2">
        <div
          className={cn(
            `
              flex items-center gap-1.5 font-mono text-xs font-medium
              tracking-wide uppercase
            `,
            kpiSubtleClass(props.tone),
          )}
        >
          {props.icon && (
            <Icon name={props.icon} size={14} className="text-muted-foreground" />
          )}
          {props.label}
        </div>
        {props.window != null && (
          <span className={cn('font-mono text-[10px] tracking-wide uppercase', kpiSubtleClass(props.tone))}>
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
            className={props.tone === 'bad' ? SEVERE : WARNING}
          />
        )}
        <span className={kpiValueClass(props.tone)}>{props.value}</span>
        {props.unit && (
          <span className={cn('text-sm', kpiSubtleClass(props.tone))}>{props.unit}</span>
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
      {props.hint && (
        <div className={cn('mt-1 text-xs', kpiSubtleClass(props.tone))}>{props.hint}</div>
      )}
    </div>
  );
};
