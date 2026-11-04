import type { DashboardView } from '@/features/dashboard/DashboardToolbar';
import type { LineSeries, ValueFormat } from '@/features/dashboard/LineChart';
import type { ChartMetric } from '@/libs/dashboardMetrics';
import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { buttonVariants } from '@/components/ui/buttonVariants';
import { Card } from '@/features/dashboard/Card';
import {
  CompareToggle,
  MetricTabs,
} from '@/features/dashboard/DashboardToolbar';
import { Icon } from '@/features/dashboard/Icon';
import { KpiCard } from '@/features/dashboard/KpiCard';
import { LineChart } from '@/features/dashboard/LineChart';
import { money } from '@/features/dashboard/money';
import { WINDOWS } from '@/features/dashboard/reportWindow';
import { WindowChip } from '@/features/dashboard/WindowChip';
import {
  chartSeries,
  deltaPct,
  errorRatePct,
  formatInt,
  formatMs,
  moneyDeltaPct,
} from '@/libs/dashboardMetrics';
import { attention } from '@/libs/dashboardOverview';
import { Link } from '@/libs/I18nNavigation';
import { InferonApi } from '@/libs/InferonApi';
import { metricTone, THRESHOLDS, toneGlyphClass } from '@/libs/reportTone';
import { currentSession } from '@/libs/session';

const CHART_METRICS: ChartMetric[] = ['requests', 'cost', 'tokens', 'ttft', 'errors'];

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
    days: (WINDOWS as readonly number[]).includes(days) ? days : 30,
    compare: params.compare === '1',
    metric: CHART_METRICS.includes(params.metric as ChartMetric)
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
  const [summary, previous, series, previousSeries, waste]
    = await Promise.all([
      api.summary(view.days, 0, session.cookie),
      view.compare ? api.summary(view.days, view.days, session.cookie) : null,
      api.timeseries(view.days, 0, session.cookie),
      view.compare ? api.timeseries(view.days, view.days, session.cookie) : null,
      api.waste(view.days, session.cookie),
    ]);
  // A failing report costs its own finding, never the page.
  const payloads = await api.payloads(view.days, session.cookie).catch(() => null);
  const findings = attention({ summary, waste, payloads });

  const requestsDelta = previous ? deltaPct(summary.requests, previous.requests) : null;
  const costDelta = previous
    ? moneyDeltaPct(summary.total_cost_usd, previous.total_cost_usd)
    : null;
  const ttftDelta = previous ? deltaPct(summary.ttft_p95_ms, previous.ttft_p95_ms) : null;
  const errorRate = errorRatePct(summary.requests, summary.errors);

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

  const header = (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h1 className="text-2xl font-semibold">{t('title_bar')}</h1>
        <p className="text-sm text-muted-foreground">
          {t('title_bar_description', { days: view.days })}
        </p>
      </div>
      <div className="flex items-center gap-2">
        <WindowChip days={view.days} />
        <CompareToggle {...view} label={t('compare')} />
      </div>
    </div>
  );

  if (summary.requests === 0) {
    return (
      <>
        {header}
        <div className="mt-6">
          <Card title={t('empty_title')} icon="activity">
            <p className="text-sm text-muted-foreground">{t('empty_body')}</p>
            <div className="mt-4 flex flex-wrap gap-2">
              <Link href="/dashboard/settings" className={buttonVariants()}>
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
        lg:grid-cols-5
      "
      >
        <KpiCard
          label={t('requests')}
          icon="activity"
          value={formatInt(summary.requests)}
          window={view.days}
          delta={requestsDelta}
          definition={t('requests_help')}
        />
        <KpiCard
          label={t('total_cost')}
          icon="cost"
          value={money(summary.total_cost_usd)}
          window={view.days}
          delta={costDelta}
          goodWhen="down"
          definition={t('total_cost_help')}
        />
        <KpiCard
          label={t('error_rate')}
          icon="warning"
          value={errorRate == null ? '—' : `${errorRate.toFixed(1)}%`}
          window={view.days}
          tone={metricTone(errorRate == null ? null : errorRate / 100, THRESHOLDS.errorRate)}
          definition={t('error_rate_help')}
        />
        <KpiCard
          label={t('p95_ttft')}
          icon="latency"
          value={formatMs(summary.ttft_p95_ms)}
          window={view.days}
          delta={ttftDelta}
          definition={t('p95_ttft_help')}
          goodWhen="down"
          hint={`${t('proxy_overhead')} ${formatMs(summary.proxy_p95_ms)} · ${t('avg_ttft')} ${formatMs(summary.avg_ttft_ms)}`}
          tone={metricTone(summary.ttft_p95_ms, THRESHOLDS.ttftP95Ms)}
        />
        <KpiCard
          label={t('total_savings')}
          icon="down"
          value={money(summary.total_savings_usd)}
          window={view.days}
          hint={`${t('cache_savings')} ${money(summary.cache_savings_usd)}`}
          definition={t('total_savings_help')}
        />
      </div>

      {findings.length > 0 && (
        <div className="mt-4">
          <Card title={t('attention')} icon="warning">
            <ul className="divide-y">
              {findings.map(finding => (
                <li key={finding.kind}>
                  <Link
                    href={finding.href}
                    className="
                      flex items-center justify-between gap-4 py-2 text-sm
                      hover:text-primary
                    "
                  >
                    <span className="flex items-center gap-2">
                      <Icon
                        name="warning"
                        size={16}
                        className={toneGlyphClass(finding.severity === 0 ? 'bad' : 'warn')}
                      />
                      {t(`attention_${finding.kind}`)}
                    </span>
                    <span className="font-mono text-xs text-muted-foreground">
                      {finding.amount > 0 ? money(finding.amount) : t('attention_now')}
                    </span>
                  </Link>
                </li>
              ))}
            </ul>
          </Card>
        </div>
      )}

      <div className="mt-4">
        <Card
          title={t('usage_over_time')}
          icon="activity"
          action={<MetricTabs {...view} labels={metricLabels} />}
        >
          <LineChart
            labels={series.points.map(point => point.date)}
            series={chartSeriesList}
            valueFormat={VALUE_FORMAT[view.metric]}
          />
        </Card>
      </div>
    </>
  );
}
