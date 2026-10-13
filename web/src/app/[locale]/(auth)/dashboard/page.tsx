import type { DashboardView } from '@/features/dashboard/DashboardToolbar';
import type { LineSeries, ValueFormat } from '@/features/dashboard/LineChart';
import type { SpendBreakdownRow } from '@/features/dashboard/SpendBreakdown';
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
import { SpendBreakdown } from '@/features/dashboard/SpendBreakdown';
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

type SearchParams = { days?: string; compare?: string; metric?: string };

const parseView = (params: SearchParams): DashboardView => {
  const days = Number(params.days) as DashboardView['days'];
  return {
    days: (RANGES as readonly number[]).includes(days) ? days : 30,
    compare: params.compare === '1',
    metric: METRICS.includes(params.metric as ChartMetric)
      ? (params.metric as ChartMetric)
      : 'requests',
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
  const [summary, previous, series, previousSeries, byModel, byPrompt, waste, keys, byProvider, prevByProvider, byKey, prevByKey] = await Promise.all([
    api.summary(view.days, 0, session.cookie),
    view.compare ? api.summary(view.days, view.days, session.cookie) : null,
    api.timeseries(view.days, 0, session.cookie),
    view.compare ? api.timeseries(view.days, view.days, session.cookie) : null,
    api.byModel(view.days, session.cookie),
    api.byPrompt(view.days, 10, session.cookie),
    api.waste(view.days, session.cookie),
    api.keys(session.cookie),
    api.byProvider(view.days, 0, session.cookie),
    view.compare ? api.byProvider(view.days, view.days, session.cookie) : null,
    api.byKey(view.days, 0, session.cookie),
    view.compare ? api.byKey(view.days, view.days, session.cookie) : null,
  ]);
  const canEdit = keys.role === 'owner';

  // ITEM-047: match the current window's rows to the prior period by id (falling
  // back to the label for the shared/legacy rows) so each row can show a delta
  // and an inline spike badge (AC3).
  const prevProviderCost = new Map(
    (prevByProvider?.providers ?? []).map(p => [p.provider_id ?? p.name, p.cost_usd]),
  );
  const providerRows: SpendBreakdownRow[] = byProvider.providers.map(p => ({
    id: p.provider_id,
    label: p.name,
    meta: p.kind === 'shared' ? undefined : p.kind,
    costUsd: p.cost_usd,
    previousCostUsd: prevProviderCost.get(p.provider_id ?? p.name) ?? null,
    requests: p.requests,
    tokens: p.prompt_tokens + p.completion_tokens,
    limitCents: p.spend_limit_cents,
    errorRate: p.error_rate,
    p95Ms: p.ttft_p95_ms,
  }));

  const prevKeyCost = new Map((prevByKey?.keys ?? []).map(k => [k.key_id ?? k.label, k.cost_usd]));
  const keyRows: SpendBreakdownRow[] = byKey.keys.map(k => ({
    id: k.key_id,
    label: k.label,
    costUsd: k.cost_usd,
    previousCostUsd: prevKeyCost.get(k.key_id ?? k.label) ?? null,
    requests: k.requests,
    tokens: k.prompt_tokens + k.completion_tokens,
    limitCents: k.spend_limit_cents,
  }));

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
              <Link
                href="/dashboard/models"
                className={buttonVariants({ variant: 'outline' })}
              >
                {t('empty_models')}
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
          hint={`${t('avg_ttft')} ${formatMs(summary.avg_ttft_ms)}`}
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
        <Card title={t('by_model')}>
          <BarChart
            rows={byModel.models.map(model => ({
              label: model.model,
              value: Number(model.cost_usd),
              display: formatUsd(model.cost_usd),
            }))}
            empty={t('no_data')}
          />
          {byModel.models.length > 0 && (
            <table className="mt-4 w-full text-left text-sm">
              <thead>
                <tr className="border-b text-muted-foreground">
                  <th className="
                    py-2 font-mono text-xs font-medium tracking-wide uppercase
                  "
                  >
                    {t('model')}
                  </th>
                  <th className="
                    py-2 text-right font-mono text-xs font-medium tracking-wide
                    uppercase
                  "
                  >
                    {t('requests')}
                  </th>
                  <th className="
                    py-2 text-right font-mono text-xs font-medium tracking-wide
                    uppercase
                  "
                  >
                    {t('tokens')}
                  </th>
                  <th className="
                    py-2 text-right font-mono text-xs font-medium tracking-wide
                    uppercase
                  "
                  >
                    {t('cached')}
                  </th>
                  <th className="
                    py-2 text-right font-mono text-xs font-medium tracking-wide
                    uppercase
                  "
                  >
                    {t('cost')}
                  </th>
                </tr>
              </thead>
              <tbody>
                {byModel.models.map(model => (
                  <tr
                    key={model.model}
                    className="
                      border-b
                      last:border-0
                    "
                  >
                    <td className="py-2">{model.model}</td>
                    <td className="py-2 text-right tabular-nums">{formatInt(model.requests)}</td>
                    <td className="py-2 text-right tabular-nums">
                      {formatCompact(model.prompt_tokens + model.completion_tokens)}
                    </td>
                    <td className="
                      py-2 text-right text-muted-foreground tabular-nums
                    "
                    >
                      {model.prompt_tokens > 0
                        ? `${Math.round((model.cached_prompt_tokens / model.prompt_tokens) * 100)}%`
                        : '—'}
                    </td>
                    <td className="py-2 text-right tabular-nums">{formatUsd(model.cost_usd)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Card>

        <Card title={t('waste')}>
          {wasteRows.length === 0
            ? (
                <p className="text-sm text-muted-foreground">{t('no_data')}</p>
              )
            : (
                <ul className="space-y-3">
                  {wasteRows.map(([flag, row]) => (
                    <li
                      key={flag}
                      className="
                        flex items-center justify-between gap-4 text-sm
                      "
                    >
                      <span className="font-medium">{wasteLabels[flag] ?? flag}</span>
                      <span className="text-muted-foreground">
                        {t('requests')}
                        {': '}
                        {formatInt(row.requests)}
                      </span>
                      <span className="tabular-nums">{formatUsd(row.spend_usd)}</span>
                      <span className="
                        text-emerald-600 tabular-nums
                        dark:text-emerald-400
                      "
                      >
                        {`−${formatUsd(row.savings_est_usd)}`}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
        </Card>
      </div>

      <div className="
        mt-4 grid grid-cols-1 gap-4
        lg:grid-cols-2
      "
      >
        <Card title={t('by_provider')}>
          <SpendBreakdown
            kind="provider"
            rows={providerRows}
            canEdit={canEdit}
            emptyLabel={t('no_data')}
          />
        </Card>

        <Card title={t('by_key')}>
          <SpendBreakdown
            kind="key"
            rows={keyRows}
            canEdit={canEdit}
            emptyLabel={t('no_data')}
          />
        </Card>
      </div>

      <div className="mt-4">
        <Card title={t('by_prompt')}>
          {byPrompt.prompts.length === 0
            ? (
                <p className="text-sm text-muted-foreground">{t('no_data')}</p>
              )
            : (
                <table className="w-full text-left text-sm">
                  <thead>
                    <tr className="border-b text-muted-foreground">
                      <th className="
                        py-2 font-mono text-xs font-medium tracking-wide
                        uppercase
                      "
                      >
                        {t('prompt_hash')}
                      </th>
                      <th className="
                        py-2 text-right font-mono text-xs font-medium
                        tracking-wide uppercase
                      "
                      >
                        {t('requests')}
                      </th>
                      <th className="
                        py-2 text-right font-mono text-xs font-medium
                        tracking-wide uppercase
                      "
                      >
                        {t('prompt_tokens')}
                      </th>
                      <th className="
                        py-2 text-right font-mono text-xs font-medium
                        tracking-wide uppercase
                      "
                      >
                        {t('cost')}
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {byPrompt.prompts.map(prompt => (
                      <tr
                        key={prompt.system_prompt_hash}
                        className="
                          border-b
                          last:border-0
                        "
                      >
                        <td className="py-2 font-mono text-xs">
                          {prompt.system_prompt_hash}
                        </td>
                        <td className="py-2 text-right tabular-nums">
                          {formatInt(prompt.requests)}
                        </td>
                        <td className="py-2 text-right tabular-nums">
                          {formatCompact(prompt.prompt_tokens)}
                        </td>
                        <td className="py-2 text-right tabular-nums">
                          {formatUsd(prompt.cost_usd)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
        </Card>
      </div>
    </>
  );
}
