// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import type { ChartMetric } from '@/libs/dashboardMetrics';
import { CHART_METRICS } from '@/libs/dashboardMetrics';
import { Link } from '@/libs/I18nNavigation';

export type DashboardView = {
  days: number;
  compare: boolean;
  metric: ChartMetric;
  hideZero: boolean;
};

/** Serialize the view into the dashboard URL (omitting defaults keeps it short). */
const href = (view: DashboardView) => ({
  pathname: '/dashboard',
  query: {
    days: view.days,
    ...(view.compare ? { compare: 1 } : {}),
    ...(view.metric !== 'requests' ? { metric: view.metric } : {}),
    ...(view.hideZero ? { hide_zero: 1 } : {}),
  },
});

const pill = (active: boolean): string =>
  active
    ? 'rounded-md bg-primary px-2.5 py-1 font-medium text-primary-foreground'
    : `
      rounded-md px-2.5 py-1 text-muted-foreground
      hover:bg-muted hover:text-foreground
    `;

export const CompareToggle = (props: DashboardView & { label: string }) => (
  <Link
    href={href({ ...props, compare: !props.compare })}
    className={`
      inline-flex items-center gap-1.5 border px-3 py-1 text-sm shadow-sm
      ${
  props.compare
    ? 'rounded-md bg-secondary font-medium text-secondary-foreground'
    : `
      rounded-md bg-card text-muted-foreground
      hover:text-foreground
    `
  }
    `}
  >
    <span aria-hidden>⇆</span>
    {props.label}
  </Link>
);

export const MetricTabs = (
  props: DashboardView & { labels: Record<ChartMetric, string> },
) => (
  <div className="flex flex-wrap gap-1 text-sm">
    {CHART_METRICS.map(metric => (
      <Link
        key={metric}
        href={href({ ...props, metric })}
        className={pill(metric === props.metric)}
      >
        {props.labels[metric]}
      </Link>
    ))}
  </div>
);
