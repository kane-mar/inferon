import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { AutoRefresh } from '@/features/dashboard/AutoRefresh';
import { Card } from '@/features/dashboard/Card';
import { PageHeader } from '@/features/shell/PageHeader';
import { formatInt } from '@/libs/dashboardMetrics';
import { Link } from '@/libs/I18nNavigation';
import { formatUsd, InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

const WINDOWS = [7, 30, 90] as const;

const pct = (value: number | null): string =>
  (value == null ? '—' : `${(value * 100).toFixed(1)}%`);

const pill = (active: boolean): string =>
  active
    ? 'rounded-[5px] bg-primary px-2.5 py-1 font-medium text-primary-foreground'
    : `
      rounded-[5px] px-2.5 py-1 text-muted-foreground
      hover:bg-muted hover:text-foreground
    `;

/** One named finding: what we detect, the live headline, and the supporting numbers. */
const Finding = (props: {
  title: string;
  description: string;
  highlight: string;
  metrics: { label: string; value: string }[];
}) => (
  <Card
    title={props.title}
    action={<span className="text-lg font-semibold tabular-nums">{props.highlight}</span>}
  >
    <p className="text-sm text-muted-foreground">{props.description}</p>
    <dl className="mt-4 space-y-1.5 text-sm">
      {props.metrics.map(metric => (
        <div
          key={metric.label}
          className="flex items-baseline justify-between gap-3"
        >
          <dt className="text-muted-foreground">{metric.label}</dt>
          <dd className="truncate text-right tabular-nums">{metric.value}</dd>
        </div>
      ))}
    </dl>
  </Card>
);

/**
 * The leak report (guides/inferon-leak-report.md) as a live page: the six named
 * findings, each computed from the tenant's own telemetry and refreshed
 * server-side every 15 seconds.
 */
export default async function LeaksPage(props: {
  params: Promise<{ locale: string }>;
  searchParams: Promise<{ days?: string }>;
}) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const requested = Number((await props.searchParams).days);
  const days = (WINDOWS as readonly number[]).includes(requested) ? requested : 30;
  const t = await getTranslations({ locale, namespace: 'LeaksPage' });
  const api = InferonApi.fromEnv();

  const [summary, waste, quality, byModel, byProvider, byKey] = await Promise.all([
    api.summary(days, 0, session.cookie),
    api.waste(days, session.cookie),
    api.quality(days, session.cookie),
    api.byModel(days, session.cookie),
    api.byProvider(days, 0, session.cookie),
    api.byKey(days, 0, session.cookie),
  ]);

  const over = waste.over_provisioned;
  const misuse = waste.model_misuse;
  const cached = waste.cache_miss;
  const overall = quality.overall;
  const topModel = byModel.models[0];
  const topProvider = byProvider.providers[0];
  const topKey = byKey.keys[0];
  const topError = Object.entries(overall.errors_by_class).sort((a, b) => b[1] - a[1])[0];

  const named = (label: string | undefined, cost: string | undefined): string =>
    label ? `${label} · ${formatUsd(cost ?? '0')}` : t('none');

  return (
    <>
      <div className="flex flex-wrap items-start justify-between gap-4">
        <PageHeader
          title={t('title_bar')}
          description={t('title_bar_description', { days })}
        />
        <div className="flex items-center gap-3">
          <AutoRefresh label={t('live')} />
          <div className="
            inline-flex items-center gap-0.5 rounded-md border bg-card p-0.5
            text-sm shadow-sm
          "
          >
            {WINDOWS.map(window => (
              <Link
                key={window}
                href={{ pathname: '/dashboard/leaks', query: { days: window } }}
                className={pill(window === days)}
              >
                {`${window}d`}
              </Link>
            ))}
          </div>
        </div>
      </div>

      {summary.requests === 0 && (
        <div className="mb-4">
          <Card title={t('no_traffic_title')}>
            <p className="text-sm text-muted-foreground">{t('no_traffic_body')}</p>
            <Link
              href="/dashboard/onboarding"
              className="
                mt-3 inline-block text-sm font-medium text-primary underline
                underline-offset-4
              "
            >
              {t('no_traffic_cta')}
            </Link>
          </Card>
        </div>
      )}

      <div className="
        grid grid-cols-1 gap-4
        lg:grid-cols-2
      "
      >
        <Finding
          title={t('f1_title')}
          description={t('f1_desc')}
          highlight={formatUsd(over?.savings_est_usd ?? '0')}
          metrics={[
            { label: t('requests'), value: formatInt(over?.requests ?? 0) },
            { label: t('spend'), value: formatUsd(over?.spend_usd ?? '0') },
            { label: t('recoverable'), value: formatUsd(over?.savings_est_usd ?? '0') },
          ]}
        />
        <Finding
          title={t('f2_title')}
          description={t('f2_desc')}
          highlight={formatUsd(misuse?.savings_est_usd ?? '0')}
          metrics={[
            { label: t('requests'), value: formatInt(misuse?.requests ?? 0) },
            { label: t('spend'), value: formatUsd(misuse?.spend_usd ?? '0') },
            { label: t('recoverable'), value: formatUsd(misuse?.savings_est_usd ?? '0') },
          ]}
        />
        <Finding
          title={t('f3_title')}
          description={t('f3_desc')}
          highlight={formatUsd(cached?.spend_usd ?? '0')}
          metrics={[
            { label: t('requests'), value: formatInt(cached?.requests ?? 0) },
            { label: t('spend'), value: formatUsd(cached?.spend_usd ?? '0') },
          ]}
        />
        <Finding
          title={t('f4_title')}
          description={t('f4_desc')}
          highlight={formatUsd(summary.total_savings_usd)}
          metrics={[
            { label: t('router'), value: formatUsd(summary.router_savings_usd) },
            { label: t('fallback'), value: formatUsd(summary.fallback_savings_usd) },
            { label: t('cache'), value: formatUsd(summary.cache_savings_usd) },
            { label: t('affinity_requests'), value: formatInt(summary.cache_affinity_requests) },
          ]}
        />
        <Finding
          title={t('f5_title')}
          description={t('f5_desc')}
          highlight={formatUsd(summary.total_cost_usd)}
          metrics={[
            { label: t('top_model'), value: named(topModel?.model, topModel?.cost_usd) },
            { label: t('top_provider'), value: named(topProvider?.name, topProvider?.cost_usd) },
            { label: t('top_key'), value: named(topKey?.label, topKey?.cost_usd) },
          ]}
        />
        <Finding
          title={t('f6_title')}
          description={t('f6_desc')}
          highlight={pct(overall.error_rate)}
          metrics={[
            { label: t('truncated_rate'), value: pct(overall.truncated_rate) },
            { label: t('guard_fallback_rate'), value: pct(overall.guard_fallback_rate) },
            {
              label: t('top_error_class'),
              value: topError ? `${topError[0]} · ${formatInt(topError[1])}` : t('none'),
            },
          ]}
        />
      </div>
    </>
  );
}
