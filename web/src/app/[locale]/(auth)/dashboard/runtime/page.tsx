import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { KpiCard } from '@/features/dashboard/KpiCard';
import {
  CELL,
  HEAD,
  heatCellClass,
  integer,
  ms,
  rate,
  us,
} from '@/features/dashboard/reportFormat';
import { Status } from '@/features/dashboard/ReportStatus';
import { PageHeader } from '@/features/shell/PageHeader';
import { InferonApi } from '@/libs/InferonApi';
import { metricTone, THRESHOLDS } from '@/libs/reportTone';
import { currentSession } from '@/libs/session';

const DAYS = 30;

/**
 * Runtime (ITEM-092/099/103): is the proxy fast? The contract, the proxy's own
 * overhead, the end-to-end latency around it, the model × time matrix that shows
 * degradation, and the per-model split. Payload governance is its own tab.
 */
export default async function RuntimePage(props: {
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({ locale, namespace: 'RuntimePage' });
  const data = await InferonApi.fromEnv().runtime(DAYS, session.cookie);
  const { overhead, contract, latency } = data;
  const hasSamples = contract.samples >= contract.min_samples;

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
          tone={metricTone(overhead.p50_us, THRESHOLDS.proxyP50Us)}
          hint={contract.p50_met == null
            ? t('not_enough_samples', { count: contract.min_samples })
            : contract.p50_met
              ? t('within_contract')
              : t('over_contract')}
        />
        <KpiCard
          label={t('overhead_p95')}
          value={us(overhead.p95_us)}
          tone={metricTone(overhead.p95_us, THRESHOLDS.proxyP95Us)}
        />
        <KpiCard
          label={t('under_1ms')}
          value={rate(overhead.under_1ms_rate)}
          tone={metricTone(overhead.under_1ms_rate, THRESHOLDS.under1msRate)}
        />
        <KpiCard label={t('samples')} value={integer(overhead.samples)} />
      </div>

      <div className="mt-4">
        <Card title={t('claim')} icon="ok">
          <div className="space-y-3">
            <div className="flex items-center justify-between gap-3 text-sm">
              <span>{t('claim_p50')}</span>
              <Status ok={hasSamples ? contract.p50_met : null} met={t('met')} missed={t('not_met')} />
            </div>
            <div className="flex items-center justify-between gap-3 text-sm">
              <span>{t('claim_p95')}</span>
              <Status ok={hasSamples ? contract.p95_met : null} met={t('met')} missed={t('not_met')} />
            </div>
            <p className="text-xs text-muted-foreground">
              {t('claim_window', {
                hours: contract.window_hours,
                samples: integer(contract.samples),
                observed: us(contract.observed_p50_us),
              })}
            </p>
            <p className="text-xs text-muted-foreground">{t('claim_note')}</p>
          </div>
        </Card>
      </div>

      {/* End to end, so the proxy's share is visible. */}
      <div className="mt-4">
        <Card title={t('end_to_end')} icon="latency">
          <div className="
            grid grid-cols-2 gap-3
            lg:grid-cols-3
          "
          >
            <KpiCard label={t('total_p50')} value={ms(latency.total_p50_ms)} />
            <KpiCard
              label={t('total_p95')}
              value={ms(latency.total_p95_ms)}
              tone={metricTone(latency.total_p95_ms, THRESHOLDS.totalP95Ms)}
            />
            <KpiCard label={t('total_p99')} value={ms(latency.total_p99_ms)} />
            <KpiCard label={t('ttft_p50')} value={ms(latency.ttft_p50_ms)} />
            <KpiCard
              label={t('ttft_p95')}
              value={ms(latency.ttft_p95_ms)}
              tone={metricTone(latency.ttft_p95_ms, THRESHOLDS.ttftP95Ms)}
            />
            <KpiCard
              label={t('proxy_share')}
              value={rate(latency.proxy_share)}
              hint={t('proxy_share_help')}
            />
          </div>
        </Card>
      </div>

      {/* Model × time matrix: degradation at a glance (ITEM-099). Empty cells
          are "no traffic" — visually distinct from a fast/healthy day. */}
      {data.heatmap.models.length > 0 && (
        <div className="mt-4">
          <Card
            title={t('heatmap', { step: data.heatmap.step })}
            icon="activity"
            action={(
              <div className="flex gap-2 text-xs">
                <a
                  className="underline underline-offset-4"
                  href={`?days=${data.window.days}&step=day`}
                >
                  {t('heatmap_day')}
                </a>
                <a
                  className="underline underline-offset-4"
                  href={`?days=${data.window.days}&step=hour`}
                >
                  {t('heatmap_hour')}
                </a>
              </div>
            )}
          >
            <p className="mb-3 text-xs text-muted-foreground">{t('heatmap_help')}</p>
            <div className="overflow-x-auto">
              <table className="w-full text-left text-xs">
                <thead>
                  <tr className="border-b text-muted-foreground">
                    <th className="py-2 pr-3 font-mono uppercase">{t('heatmap_model')}</th>
                    {data.heatmap.buckets.map(bucket => (
                      <th key={bucket} className="p-2 text-right font-mono">
                        {bucket.slice(5)}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {data.heatmap.models.map(row => (
                    <tr
                      key={row.model}
                      className="
                        border-b
                        last:border-0
                      "
                    >
                      <td className="py-2 pr-3 font-mono">{row.model}</td>
                      {row.cells.map((cell, index) => (
                        <td
                          key={`${row.model}-${data.heatmap.buckets[index]}`}
                          className={`
                            p-2 text-right font-mono tabular-nums
                            ${heatCellClass(cell)}
                          `}
                          title={cell
                            ? `${cell.requests} request(s), ${cell.errors} error(s)`
                            : t('heatmap_empty')}
                        >
                          {cell
                            ? (
                                <>
                                  {cell.p95_ms == null ? '—' : `${Math.round(cell.p95_ms)}ms`}
                                  <span className="ml-1 opacity-70">
                                    {cell.error_rate == null
                                      ? ''
                                      : `${(cell.error_rate * 100).toFixed(0)}%`}
                                  </span>
                                </>
                              )
                            : '·'}
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </Card>
        </div>
      )}

      {/* Where the overhead goes. */}
      <div className="mt-4">
        <Card title={t('by_model')} icon="runtime">
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

export const dynamic = 'force-dynamic';
