import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { KpiCard } from '@/features/dashboard/KpiCard';
import { PageHeader } from '@/features/shell/PageHeader';
import { formatUsd, InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

const HEAD = 'px-3 py-2 text-left font-mono text-xs uppercase text-muted-foreground';
const CELL = 'px-3 py-2 text-sm tabular-nums';
const DAYS = 30;

const us = (value: number | null) =>
  value == null ? '—' : `${value.toLocaleString('en-US')} µs`;

const ms = (value: number | null) =>
  value == null ? '—' : `${Math.round(value).toLocaleString('en-US')} ms`;

const rate = (value: number | null) =>
  value == null ? '—' : `${(value * 100).toFixed(1)}%`;

const integer = (value: number) => value.toLocaleString('en-US');
// `formatUsd` expects a decimal string; sessions return them as strings already.
const usd = (value: string | null) => (value == null ? '—' : formatUsd(value));

/** A small met / not-met marker (null = no samples yet). */
const Status = (props: { ok: boolean | null; met: string; missed: string }) => {
  if (props.ok == null) {
    return <span className="text-sm text-muted-foreground">—</span>;
  }
  return (
    <span className={props.ok
      ? `
        text-sm font-medium text-emerald-600
        dark:text-emerald-400
      `
      : `
        text-sm font-medium text-amber-600
        dark:text-amber-400
      `}
    >
      {props.ok ? props.met : props.missed}
    </span>
  );
};

/**
 * Performance (ITEM-092): the proxy's own overhead against its sub-millisecond
 * contract, then end-to-end latency, the hot-path cache and the per-model split.
 * The claim is at the top because it is the number that can be wrong.
 */
export default async function PerformancePage(props: {
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({ locale, namespace: 'PerformancePage' });
  const api = InferonApi.fromEnv();
  const [data, sessions] = await Promise.all([
    api.performance(DAYS, session.cookie),
    api.sessions(DAYS, session.cookie),
  ]);
  const { overhead, contract, latency, cache } = data;
  const hasSamples = overhead.samples > 0;

  return (
    <>
      <PageHeader title={t('title_bar')} description={t('title_bar_description')} />

      {/* The headline: what the proxy itself adds. */}
      <div className="
        mt-6 grid grid-cols-2 gap-3
        lg:grid-cols-4
      "
      >
        <KpiCard
          label={t('overhead_p50')}
          value={us(overhead.p50_us)}
          hint={contract.p50_met == null
            ? t('not_enough_samples', { count: contract.min_samples })
            : contract.p50_met
              ? t('within_contract')
              : t('over_contract')}
        />
        <KpiCard label={t('overhead_p95')} value={us(overhead.p95_us)} />
        <KpiCard label={t('under_1ms')} value={rate(overhead.under_1ms_rate)} />
        <KpiCard label={t('samples')} value={integer(overhead.samples)} />
      </div>

      <div className="mt-4">
        <Card title={t('claim')}>
          <div className="space-y-3">
            <div className="flex items-center justify-between gap-3 text-sm">
              <span>{t('claim_p50')}</span>
              <Status
                ok={hasSamples ? contract.p50_met : null}
                met={t('met')}
                missed={t('not_met')}
              />
            </div>
            <div className="flex items-center justify-between gap-3 text-sm">
              <span>{t('claim_p95')}</span>
              <Status
                ok={hasSamples ? contract.p95_met : null}
                met={t('met')}
                missed={t('not_met')}
              />
            </div>
            <p className="text-xs text-muted-foreground">{t('claim_note')}</p>
          </div>
        </Card>
      </div>

      {/* End to end, so the proxy's share is visible. */}
      <div className="mt-4">
        <Card title={t('end_to_end')}>
          <div className="
            grid grid-cols-2 gap-3
            lg:grid-cols-6
          "
          >
            <KpiCard label={t('total_p50')} value={ms(latency.total_p50_ms)} />
            <KpiCard label={t('total_p95')} value={ms(latency.total_p95_ms)} />
            <KpiCard label={t('total_p99')} value={ms(latency.total_p99_ms)} />
            <KpiCard label={t('ttft_p50')} value={ms(latency.ttft_p50_ms)} />
            <KpiCard label={t('ttft_p95')} value={ms(latency.ttft_p95_ms)} />
            <KpiCard
              label={t('proxy_share')}
              value={rate(latency.proxy_share)}
              hint={t('proxy_share_help')}
            />
          </div>
        </Card>
      </div>

      {/* The unit of work: what one client transaction actually costs. */}
      <div className="mt-4">
        <Card title={t('transactions')}>
          <div className="grid grid-cols-3 gap-3">
            <KpiCard
              label={t('transactions_p50')}
              value={usd(sessions.distribution.p50_cost_usd)}
            />
            <KpiCard
              label={t('transactions_p95')}
              value={usd(sessions.distribution.p95_cost_usd)}
            />
            <KpiCard
              label={t('transactions_count')}
              value={integer(sessions.distribution.sessions)}
            />
          </div>
          <p className="mt-3 text-xs text-muted-foreground">{t('transactions_help')}</p>
          {sessions.sessions.length > 0 && (
            <div className="mt-3 overflow-x-auto">
              <table className="w-full">
                <thead>
                  <tr className="border-b">
                    <th className={HEAD}>{t('transaction')}</th>
                    <th className={HEAD}>{t('turns')}</th>
                    <th className={HEAD}>{t('models')}</th>
                    <th className={HEAD}>{t('cost')}</th>
                    <th className={HEAD}>{t('errors')}</th>
                  </tr>
                </thead>
                <tbody>
                  {sessions.sessions.slice(0, 5).map(row => (
                    <tr
                      key={row.session}
                      className="
                        border-b
                        last:border-0
                      "
                    >
                      <td className={`
                        ${CELL}
                        font-mono text-xs
                      `}
                      >
                        {row.session}
                      </td>
                      <td className={CELL}>{integer(row.turns)}</td>
                      <td className={CELL}>{integer(row.models)}</td>
                      <td className={CELL}>{usd(row.cost_usd)}</td>
                      <td className={CELL}>{integer(row.errors)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Card>
      </div>

      {/* Advisory findings: waste the proxy can see before the call leaves. */}
      <div className="mt-4">
        <Card title={t('findings')}>
          <p className="mb-3 text-xs text-muted-foreground">{t('findings_help')}</p>
          {data.lint.requests === 0
            ? (
                <p className="text-sm text-muted-foreground">{t('findings_none')}</p>
              )
            : (
                <div className="flex flex-wrap items-center gap-4">
                  <span className="text-sm">
                    {t('findings_count', { count: data.lint.requests })}
                  </span>
                  <span className="font-mono text-xs text-muted-foreground">
                    {t('findings_waste', { usd: data.lint.wasted_usd })}
                  </span>
                  {data.lint.dismissed > 0 && (
                    <span className="font-mono text-xs text-muted-foreground">
                      {t('findings_dismissed', { count: data.lint.dismissed })}
                    </span>
                  )}
                  <div className="flex flex-wrap gap-2">
                    {Object.entries(data.lint.by_kind).map(([kind, count]) => (
                      <span
                        key={kind}
                        className="
                          rounded-md border px-2 py-1 font-mono text-xs
                          text-muted-foreground
                        "
                      >
                        {kind}
                        {' '}
                        ·
                        {integer(count)}
                      </span>
                    ))}
                  </div>
                </div>
              )}
        </Card>
      </div>

      {/* Operator-only: process-wide cache counters. */}
      {cache && (
        <div className="mt-4">
          <Card title={t('cache')}>
            <p className="mb-3 text-xs text-muted-foreground">{t('cache_note')}</p>
            <div className="overflow-x-auto">
              <table className="w-full">
                <thead>
                  <tr className="border-b">
                    <th className={HEAD}>{t('cache_kind')}</th>
                    <th className={HEAD}>{t('hits')}</th>
                    <th className={HEAD}>{t('misses')}</th>
                    <th className={HEAD}>{t('hit_rate')}</th>
                  </tr>
                </thead>
                <tbody>
                  {(
                    [
                      ['auth', cache.auth],
                      ['config', cache.config],
                      ['route', cache.route],
                      ['warm', cache.warm],
                      ['spend', cache.spend],
                    ] as const
                  ).map(([kind, count]) => (
                    <tr
                      key={kind}
                      className="
                        border-b
                        last:border-0
                      "
                    >
                      <td className={`
                        ${CELL}
                        font-mono
                      `}
                      >
                        {t(`cache_${kind}`)}
                      </td>
                      <td className={CELL}>{integer(count.hits)}</td>
                      <td className={CELL}>{integer(count.misses)}</td>
                      <td className={CELL}>{rate(count.hit_rate)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <p className="mt-3 text-xs text-muted-foreground">
              {t('cache_tail', {
                invalidations: integer(cache.invalidations),
                tenants: integer(cache.tenants),
              })}
            </p>
          </Card>
        </div>
      )}

      {/* Where the overhead goes. */}
      <div className="mt-4">
        <Card title={t('by_model')}>
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead>
                <tr className="border-b">
                  <th className={HEAD}>{t('model')}</th>
                  <th className={HEAD}>{t('requests')}</th>
                  <th className={HEAD}>{t('overhead_p50')}</th>
                  <th className={HEAD}>{t('overhead_p95')}</th>
                  <th className={HEAD}>{t('total_p50')}</th>
                  <th className={HEAD}>{t('total_p95')}</th>
                </tr>
              </thead>
              <tbody>
                {data.by_model.map(row => (
                  <tr
                    key={row.model}
                    className="
                      border-b
                      last:border-0
                    "
                  >
                    <td className={`
                      ${CELL}
                      font-mono
                    `}
                    >
                      {row.model}
                    </td>
                    <td className={CELL}>{integer(row.requests)}</td>
                    <td className={CELL}>{us(row.proxy_p50_us)}</td>
                    <td className={CELL}>{us(row.proxy_p95_us)}</td>
                    <td className={CELL}>{ms(row.total_p50_ms)}</td>
                    <td className={CELL}>{ms(row.total_p95_ms)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {data.by_model.length === 0 && (
              <p className="text-sm text-muted-foreground">{t('no_samples')}</p>
            )}
          </div>
        </Card>
      </div>
    </>
  );
}
