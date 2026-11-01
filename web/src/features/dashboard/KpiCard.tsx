import type { Tone } from '@/libs/reportTone';
import { trend } from '@/libs/dashboardMetrics';
import { kpiCardClass, kpiDeltaClass, kpiSubtleClass } from '@/libs/reportTone';
import { cn } from '@/utils/Helpers';

/**
 * Headline metric with an optional period-over-period delta. `goodWhen` marks
 * which direction is an improvement (spend/down, requests/up).
 *
 * `tone` tints the whole box (background, border, contrasting text): a report
 * says whether the number is healthy, not just what it is (`metricTone`).
 */
export const KpiCard = (props: {
  label: string;
  value: string;
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
    <div className={kpiCardClass(props.tone)}>
      <div
        className={cn(
          'font-mono text-xs font-medium tracking-wide uppercase',
          kpiSubtleClass(props.tone),
        )}
      >
        {props.label}
      </div>
      <div className="mt-2 flex items-baseline gap-2">
        <span className="text-2xl font-semibold tabular-nums">{props.value}</span>
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
