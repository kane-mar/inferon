import { trend } from '@/libs/dashboardMetrics';

const GOOD = 'text-emerald-600 dark:text-emerald-400';
const BAD = 'text-destructive';

/**
 * Headline metric with an optional period-over-period delta. `goodWhen` marks
 * which direction is an improvement (spend/down, requests/up).
 */
export const KpiCard = (props: {
  label: string;
  value: string;
  delta?: number | null;
  goodWhen?: 'up' | 'down';
  hint?: string;
}) => {
  const delta = props.delta ?? null;
  const direction = trend(delta);
  const improved
    = direction === 'flat'
      ? null
      : ((props.goodWhen ?? 'up') === 'up') === (direction === 'up');

  return (
    <div className="
      rounded-lg border bg-card p-4 text-card-foreground shadow-sm
    "
    >
      <div className="
        text-xs font-medium tracking-wide text-muted-foreground uppercase
      "
      >
        {props.label}
      </div>
      <div className="mt-2 flex items-baseline gap-2">
        <span className="text-2xl font-semibold tabular-nums">{props.value}</span>
        {delta != null && direction !== 'flat' && (
          <span
            className={`
              text-xs font-medium
              ${improved ? GOOD : BAD}
            `}
            title={props.goodWhen === 'down' ? 'lower is better' : 'higher is better'}
          >
            <span aria-hidden>{direction === 'up' ? '▲' : '▼'}</span>
            {` ${Math.abs(delta).toFixed(1)}%`}
          </span>
        )}
      </div>
      {props.hint && (
        <div className="mt-1 text-xs text-muted-foreground">{props.hint}</div>
      )}
    </div>
  );
};
