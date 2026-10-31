import type { DashboardView } from '@/features/dashboard/DashboardToolbar';
import type { LineSeries, ValueFormat } from '@/features/dashboard/LineChart';
import type { ChartMetric } from '@/libs/dashboardMetrics';
import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { buttonVariants } from '@/components/ui/buttonVariants';
import { BarChart } from '@/features/dashboard/BarChart';
import { Card } from '@/features/dashboard/Card';
import {
  CompareToggle,

  MetricTabs,
  RangeSwitcher,
} from '@/features/dashboard/DashboardToolbar';
import { KpiCard } from '@/features/dashboard/KpiCard';
import { LineChart } from '@/features/dashboard/LineChart';
import {
  chartSeries,
  deltaPct,
  errorRatePct,
  formatCompact,
  formatInt,
  formatMs,
  moneyDeltaPct,
} from '@/libs/dashboardMetrics';
import { collectOverview } from '@/libs/dashboardOverview';
import { Link } from '@/libs/I18nNavigation';
import { formatUsd, InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

const RANGES = [7, 30, 90] as const;

/** One `label · value` line inside a card. */
const MetricRow = (props: { label: string; value: string }) => (
  <div className="flex items-center justify-between gap-3">
    <dt className="text-muted-foreground">{props.label}</dt>
    <dd className="tabular-nums">{props.value}</dd>
  </div>
);
const METRICS: ChartMetric[] = ['requests', 'cost', 'tokens', 'ttft', 'errors'];

const VALUE_FORMAT: Record<ChartMetric, ValueFormat> = {
  requests: 'int',
  errors: 'int',
  cost: 'usd',
  tokens: 'compact',
  ttft: 'ms',
};

type SearchParams = { days?: string; compare?: string; metric?: string; hide_zero?: string };

const parseView = (params: SearchParams): DashboardView => {
  const days = Number(params.days) as DashboardView['days'];
  return {
    days: (RANGES as readonly number[]).includes(days) ? days : 30,
    compare: params.compare === '1',
    metric: METRICS.includes(params.metric as ChartMetric)
      ? (params.metric as ChartMetric)
      : 'requests',
    hideZero: params.hide_zero === '1',
  };
};

type DashboardIndexPageProps = {
  params: Promise<{ locale: string }>;
  searchParams: Promise<SearchParams>;
};

export default async function DashboardIndexPage(props: DashboardIndexPageProps) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const view = parseView(await props.searchParams);
  const t = await getTranslations({ locale, namespace: 'DashboardIndexPage' });

  const api = InferonApi.fromEnv();
  // The single dashboard draws from every report. A report that fails costs its
  // own card, never the page (the performance model once 500'd and blanked it).
  const [summary, previous, series, previousSeries, waste, byProvider, quality]
    = await Promise.all([
      api.summary(view.days, 0, session.cookie),
      view.compare ? api.summary(view.days, view.days, session.cookie) : null,
      api.timeseries(view.days, 0, session.cookie),
      view.compare ? api.timeseries(view.days, view.days, session.cookie) : null,
      api.waste(view.days, session.cookie),
      api.byProvider(view.days, 0, session.cookie),
      api.quality(view.days, session.cookie),
    ]);
  const [performance, datasets] = await Promise.all([
    api.performance(view.days, session.cookie).catch(() => null),
    api.datasets(view.days, session.cookie).catch(() => null),
  ]);
  const overview = collectOverview({ summary, quality, performance, datasets, waste });

  // One dashboard: the headline from each report — spend, the audit's
  // recoverable waste, quality, the proxy's own overhead and the dataset the
  // traffic produced. Each card links to its full report.
  const topProviders = byProvider.providers
    .filter(provider => Number(provider.cost_usd) > 0)
    .slice(0, 3);

  const tokens = overview.tokens;
  const previousTokens = previous
    ? previous.total_prompt_tokens + previous.total_completion_tokens
    : null;
  const errorRate = overview.errorRatePct;
  const previousErrorRate = previous
    ? errorRatePct(previous.requests, previous.errors)
    : null;

  const requestsDelta = previous ? deltaPct(summary.requests, previous.requests) : null;
  const costDelta = previous
    ? moneyDeltaPct(summary.total_cost_usd, previous.total_cost_usd)
    : null;
  const avgCostDelta = previous
    ? moneyDeltaPct(summary.avg_cost_usd, previous.avg_cost_usd)
    : null;
  const tokensDelta = previous ? deltaPct(tokens, previousTokens) : null;
  const errorRateDelta = previous ? deltaPct(errorRate, previousErrorRate) : null;
  const ttftDelta = previous ? deltaPct(summary.ttft_p95_ms, previous.ttft_p95_ms) : null;

  const chartSeriesList: LineSeries[] = [
    {
      name: t('current'),
      color: 'var(--chart-1)',
      values: chartSeries(series.points, view.metric),
      area: true,
    },
  ];
  if (previousSeries) {
    chartSeriesList.push({
      name: t('previous'),
      color: 'var(--muted-foreground)',
      values: chartSeries(previousSeries.points, view.metric),
      dashed: true,
    });
  }

  const metricLabels: Record<ChartMetric, string> = {
    requests: t('requests'),
    cost: t('cost'),
    tokens: t('tokens'),
    ttft: t('ttft'),
    errors: t('errors'),
  };

  const wasteLabels: Record<string, string> = {
    over_provisioned: t('waste_over_provisioned'),
    cache_miss: t('waste_cache_miss'),
    model_misuse: t('waste_model_misuse'),
  };
  const wasteRows = Object.entries(waste);

  const header = (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h1 className="text-2xl font-semibold">{t('title_bar')}</h1>
        <p className="text-sm text-muted-foreground">
          {t('title_bar_description', { days: view.days })}
        </p>
      </div>
      <div className="flex items-center gap-2">
        <RangeSwitcher {...view} />
        <CompareToggle {...view} label={t('compare')} />
      </div>
    </div>
  );

  if (summary.requests === 0) {
    return (
      <>
        {header}
        <div className="mt-6">
          <Card title={t('empty_title')}>
            <p className="text-sm text-muted-foreground">{t('empty_body')}</p>
            <div className="mt-4 flex flex-wrap gap-2">
              <Link href="/dashboard/onboarding" className={buttonVariants()}>
                {t('empty_connect')}
              </Link>
              <Link
                href="/dashboard/providers"
                className={buttonVariants({ variant: 'outline' })}
              >
                {t('empty_provider')}
              </Link>
            </div>
          </Card>
        </div>
      </>
    );
  }

  return (
    <>
      {header}

      <div className="
        mt-6 grid grid-cols-2 gap-3
        lg:grid-cols-3
        xl:grid-cols-7
      "
      >
        <KpiCard
          label={t('requests')}
          value={formatInt(summary.requests)}
          delta={requestsDelta}
        />
        <KpiCard
          label={t('total_cost')}
          value={formatUsd(summary.total_cost_usd)}
          delta={costDelta}
          goodWhen="down"
        />
        <KpiCard
          label={t('avg_cost')}
          value={formatUsd(summary.avg_cost_usd)}
          delta={avgCostDelta}
          goodWhen="down"
        />
        <KpiCard
          label={t('error_rate')}
          value={errorRate == null ? '—' : `${errorRate.toFixed(1)}%`}
          delta={errorRateDelta}
          goodWhen="down"
        />
        <KpiCard
          label={t('tokens')}
          value={formatCompact(tokens)}
          delta={tokensDelta}
        />
        <KpiCard
          label={t('total_savings')}
          value={formatUsd(summary.total_savings_usd)}
          hint={`${t('cache_savings')} ${formatUsd(summary.cache_savings_usd)}`}
        />
        <KpiCard
          label={t('p95_ttft')}
          value={formatMs(summary.ttft_p95_ms)}
          delta={ttftDelta}
          goodWhen="down"
          hint={`${t('proxy_overhead')} ${formatMs(summary.proxy_p95_ms)} · ${t('avg_ttft')} ${formatMs(summary.avg_ttft_ms)}`}
        />
      </div>

      <div className="mt-4">
        <Card
          title={t('usage_over_time')}
          action={<MetricTabs {...view} labels={metricLabels} />}
        >
          <LineChart
            labels={series.points.map(point => point.date)}
            series={chartSeriesList}
            valueFormat={VALUE_FORMAT[view.metric]}
          />
        </Card>
      </div>

      <div className="
        mt-4 grid grid-cols-1 gap-4
        lg:grid-cols-2
      "
      >
        <Card
          title={t('top_spend')}
          action={(
            <Link
              href="/dashboard/reports/spend"
              className="text-sm text-primary"
            >
              {t('view_spend')}
            </Link>
          )}
        >
          <BarChart
            rows={topProviders.map(provider => ({
              label: provider.name,
              value: Number(provider.cost_usd),
              display: formatUsd(provider.cost_usd),
            }))}
            empty={t('no_data')}
          />
        </Card>

        <Card
          title={t('waste')}
          action={(
            <Link href="/dashboard/audit" className="text-sm text-primary">
              {t('view_audit')}
            </Link>
          )}
        >
          {wasteRows.length === 0
            ? (
                <p className="text-sm text-muted-foreground">
                  {t('no_waste', { requests: formatInt(summary.requests) })}
                </p>
              )
            : (
                <>
                  <p className="text-2xl font-semibold tabular-nums">
                    {formatUsd(overview.recoverableUsd)}
                  </p>
                  <p className="mt-1 text-sm text-muted-foreground">
                    {t('recoverable_over', { requests: formatInt(overview.leakRequests) })}
                  </p>
                  <ul className="mt-3 space-y-1.5 text-sm">
                    {wasteRows.map(([flag, row]) => (
                      <li
                        key={flag}
                        className="flex items-center justify-between gap-3"
                      >
                        <span className="text-muted-foreground">
                          {wasteLabels[flag] ?? flag}
                        </span>
                        <span className="tabular-nums">
                          {formatUsd(row.savings_est_usd)}
                        </span>
                      </li>
                    ))}
                  </ul>
                </>
              )}
        </Card>

        <Card
          title={t('performance_card')}
          action={(
            <Link href="/dashboard/performance" className="text-sm text-primary">
              {t('view_performance')}
            </Link>
          )}
        >
          {overview.performance.state !== 'ok'
            ? (
                <p className="text-sm text-muted-foreground">
                  {overview.performance.state === 'empty'
                    ? t('performance_empty')
                    : t('report_unavailable')}
                </p>
              )
            : (
                <dl className="space-y-1.5 text-sm">
                  <MetricRow
                    label={t('proxy_p50')}
                    value={overview.performance.value.proxyP50Us == null
                      ? '—'
                      : `${formatInt(overview.performance.value.proxyP50Us)} µs`}
                  />
                  <MetricRow
                    label={t('under_1ms')}
                    value={overview.performance.value.under1msRate == null
                      ? '—'
                      : `${(overview.performance.value.under1msRate * 100).toFixed(1)}%`}
                  />
                  <MetricRow
                    label={t('p95_total')}
                    value={formatMs(overview.performance.value.totalP95Ms)}
                  />
                  <MetricRow
                    label={t('p95_ttft')}
                    value={formatMs(overview.performance.value.ttftP95Ms)}
                  />
                  <MetricRow
                    label={t('cache_hit')}
                    value={overview.performance.value.cacheHitRate == null
                      ? '—'
                      : `${(overview.performance.value.cacheHitRate * 100).toFixed(1)}%`}
                  />
                </dl>
              )}
        </Card>

        <Card
          title={t('datasets_card')}
          action={(
            <Link href="/dashboard/datasets" className="text-sm text-primary">
              {t('view_datasets')}
            </Link>
          )}
        >
          {overview.datasets.state !== 'ok'
            ? (
                <p className="text-sm text-muted-foreground">
                  {overview.datasets.state === 'empty'
                    ? t('datasets_empty')
                    : t('report_unavailable')}
                </p>
              )
            : (
                <>
                  <p className="text-2xl font-semibold tabular-nums">
                    {formatInt(overview.datasets.value.examples)}
                  </p>
                  <p className="mt-1 text-sm text-muted-foreground">
                    {t('verified_examples', {
                      clusters: formatInt(overview.datasets.value.clusters),
                      grouping: overview.datasets.value.grouping === 'cluster'
                        ? t('grouping_cluster')
                        : t('grouping_hash'),
                    })}
                  </p>
                  <dl className="mt-3 space-y-1.5 text-sm">
                    <MetricRow
                      label={t('datasets_spend')}
                      value={formatUsd(overview.datasets.value.spendUsd)}
                    />
                    <MetricRow
                      label={t('datasets_excluded')}
                      value={formatInt(overview.datasets.value.excluded)}
                    />
                  </dl>
                </>
              )}
        </Card>

        <Card
          title={t('quality_card')}
          action={(
            <Link href="/dashboard/quality" className="text-sm text-primary">
              {t('view_quality')}
            </Link>
          )}
        >
          <dl className="space-y-1.5 text-sm">
            <div className="flex items-center justify-between gap-3">
              <dt className="text-muted-foreground">{t('error_rate')}</dt>
              <dd className="tabular-nums">
                {errorRate == null ? '—' : `${errorRate.toFixed(1)}%`}
              </dd>
            </div>
            <div className="flex items-center justify-between gap-3">
              <dt className="text-muted-foreground">{t('truncated')}</dt>
              <dd className="tabular-nums">
                {quality.overall.truncated_rate == null
                  ? '—'
                  : `${(quality.overall.truncated_rate * 100).toFixed(1)}%`}
              </dd>
            </div>
            <div className="flex items-center justify-between gap-3">
              <dt className="text-muted-foreground">{t('top_failure')}</dt>
              <dd className="truncate text-right tabular-nums">
                {overview.topErrorClass
                  ? `${overview.topErrorClass.name} · ${formatInt(overview.topErrorClass.count)}`
                  : t('none')}
              </dd>
            </div>
            <div className="flex items-center justify-between gap-3">
              <dt className="text-muted-foreground">{t('thumbs')}</dt>
              <dd className="tabular-nums">
                {formatInt(overview.thumbsUp)}
                {' / '}
                {formatInt(overview.thumbsDown)}
              </dd>
            </div>
          </dl>
        </Card>
      </div>
    </>
  );
}
