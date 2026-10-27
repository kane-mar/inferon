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
import { Link } from '@/libs/I18nNavigation';
import { formatUsd, InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

const RANGES = [7, 30, 90] as const;
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
  const [summary, previous, series, previousSeries, waste, byProvider, quality] = await Promise.all([
    api.summary(view.days, 0, session.cookie),
    view.compare ? api.summary(view.days, view.days, session.cookie) : null,
    api.timeseries(view.days, 0, session.cookie),
    view.compare ? api.timeseries(view.days, view.days, session.cookie) : null,
    api.waste(view.days, session.cookie),
    api.byProvider(view.days, 0, session.cookie),
    api.quality(view.days, session.cookie),
  ]);

  // The overview is the headline: top spenders, what is leaking, how the
  // responses behave. The full tables live in the spend report.
  const topProviders = byProvider.providers
    .filter(provider => Number(provider.cost_usd) > 0)
    .slice(0, 3);
  const recoverable = Object.values(waste).reduce(
    (total, row) => total + Number(row.savings_est_usd),
    0,
  );
  const leakRequests = Object.values(waste).reduce((total, row) => total + row.requests, 0);
  const topFailure = Object.entries(quality.overall.errors_by_class).sort((a, b) => b[1] - a[1])[0];

  const tokens = summary.total_prompt_tokens + summary.total_completion_tokens;
  const previousTokens = previous
    ? previous.total_prompt_tokens + previous.total_completion_tokens
    : null;
  const errorRate = errorRatePct(summary.requests, summary.errors);
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
        lg:grid-cols-3
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
            <Link href="/dashboard/leaks" className="text-sm text-primary">
              {t('view_leaks')}
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
                    {formatUsd(recoverable.toFixed(6))}
                  </p>
                  <p className="mt-1 text-sm text-muted-foreground">
                    {t('recoverable_over', { requests: formatInt(leakRequests) })}
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
                {topFailure ? `${topFailure[0]} · ${formatInt(topFailure[1])}` : t('none')}
              </dd>
            </div>
          </dl>
        </Card>
      </div>
    </>
  );
}
