// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { DocsLink } from '@/components/DocsLink';
import { Card } from '@/features/dashboard/Card';
import { KpiCard } from '@/features/dashboard/KpiCard';
import {
  CELL,
  duration,
  HEAD,
  HEAD_LEFT,
  HEAD_RIGHT,
  heatCellClass,
  micros,
  percent,
} from '@/features/dashboard/reportFormat';
import { Status } from '@/features/dashboard/ReportStatus';
import { reportDays } from '@/features/dashboard/reportWindow';
import { WindowChip } from '@/features/dashboard/WindowChip';
import { PageHeader } from '@/features/shell/PageHeader';
import { driftCellClass, driftMark } from '@/libs/bandDrift';
import { formatInt } from '@/libs/dashboardMetrics';
import { InferonApi } from '@/libs/InferonApi';
import { metricTone, THRESHOLDS } from '@/libs/reportTone';
import { currentSession } from '@/libs/session';
import { cn } from '@/utils/Helpers';

/** `2026-11-09 06` -> `09 Nov 06h`, short enough for a column of them. */
const shortBucket = (bucket: string): string => {
  const [date, hour] = bucket.split(' ');
  const [, month, day] = (date ?? '').split('-');
  return month && day ? `${day} ${month} ${hour ?? ''}h`.trim() : bucket;
};

/**
 * Runtime (ITEM-092/099/103): is the proxy fast? The contract, the proxy's own
 * overhead, the end-to-end latency around it, the model × time matrix that shows
 * degradation, and the per-model split. Payload governance is its own tab.
 */
export default async function RuntimePage(props: {
  params: Promise<{ locale: string }>;
  searchParams: Promise<{ days?: string }>;
}) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const days = reportDays(await props.searchParams);
  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({ locale, namespace: 'RuntimePage' });
  const data = await InferonApi.fromEnv().runtime(days, session.cookie);
  const { overhead, contract, latency, bands } = data;
  // ITEM-136: the claim's own basis is the small-prompt band. Everything the
  // headline says about the contract is read from that band, never from a blend
  // that averages a 4 KB prompt with a 1.2 MB one.
  // The series' own buckets: every band reports the same set, so one union does.
  const buckets = [...new Set(bands.flatMap(row => row.points.map(p => p.bucket)))].sort();
  const small = {
    band: bands.find(b => b.max_tokens === contract.basis.max_tokens)
      ?? { band: 'small', max_tokens: contract.basis.max_tokens, requests: 0, under_1ms_rate: null, p50_us: null, p95_us: null, prompt_tokens_p50: null, body_p50_ms: null },
  };
  const hasSamples = contract.samples >= contract.min_samples;

  return (
    <>
      <PageHeader
        title={t('title_bar')}
        action={(
          <div className="flex items-center gap-3">
            <DocsLink guide="config/04-auto-router-and-threshold" label={t('docs')} />
            <WindowChip days={days} />
          </div>
        )}
      />

      {/* The headline: what the proxy itself adds. */}
      <div className="
        mt-6 grid grid-cols-2 gap-3
        lg:grid-cols-4
      "
      >
        {/* ITEM-136: the headline is the claim's own basis — small prompts. A
            blended percentile over 4 KB and 1.2 MB requests describes neither,
            and comparing the blend to a 1 ms budget was the report's worst lie. */}
        <KpiCard
          label={t('claim_p50_kpi')}
          icon="latency"
          value={micros(small.band.p50_us)}
          window={days}
          tone={metricTone(small.band.p50_us, THRESHOLDS.proxyP50Us)}
          verdict={small.band.requests === 0
            ? t('no_traffic')
            : contract.p50_met == null
              ? t('too_few')
              : contract.p50_met
                ? t('within_claim')
                : t('over_claim')}
        />
        <KpiCard
          label={t('claim_p95_kpi')}
          icon="latency"
          value={micros(small.band.p95_us)}
          window={days}
          tone={metricTone(small.band.p95_us, THRESHOLDS.proxyP95Us)}
        />
        <KpiCard
          label={t('under_1ms')}
          icon="ok"
          value={percent(small.band.under_1ms_rate)}
          window={days}
          tone={metricTone(small.band.under_1ms_rate, THRESHOLDS.under1msRate)}
        />
        <KpiCard
          label={t('samples')}
          icon="activity"
          value={formatInt(overhead.samples)}
          unit={t('requests')}
          window={days}
        />
      </div>

      {/* ITEM-138: a fact about the data, not an explanation of it. */}
      {contract.basis.measured_since != null && contract.basis.excluded_requests > 0 && (
        <p className="mt-3 text-xs text-muted-foreground tabular-nums">
          {t('era_fact', {
            since: contract.basis.measured_since.slice(0, 10),
            count: formatInt(contract.basis.excluded_requests),
          })}
        </p>
      )}

      <div className="mt-6">
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
            <p className="text-xs text-muted-foreground tabular-nums">
              {t('claim_data', {
                hours: contract.window_hours,
                samples: formatInt(contract.samples),
                observed: micros(contract.observed_p50_us),
                tokens: formatInt(contract.prompt_tokens_p50 ?? 0),
              })}
            </p>
          </div>
        </Card>
      </div>

      {/* ITEM-136: one blended percentile describes neither end of the traffic.
          Each payload band is measured on its own, with the client's upload
          beside it, so "the gateway got slower" and "we are sending bigger
          prompts" cannot be confused again. */}
      <div className="mt-6">
        <Card
          title={t('bands_title')}
          icon="latency"
          action={<DocsLink guide="config/04-auto-router-and-threshold" label={t('docs')} />}
        >
          {bands.length === 0
            ? <p className="text-sm text-muted-foreground">{t('no_traffic')}</p>
            : (
                <table className="w-full text-sm">
                  <thead className="sticky top-0 z-10 bg-card">
                    <tr className="border-b text-muted-foreground">
                      <th className={HEAD_LEFT}>{t('band')}</th>
                      <th className={HEAD_RIGHT}>{t('requests')}</th>
                      <th className={HEAD_RIGHT}>{t('band_p50')}</th>
                      <th className={HEAD_RIGHT}>{t('band_p95')}</th>
                      <th className={HEAD_RIGHT}>{t('band_upload')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {bands.map(row => (
                      <tr
                        key={row.band}
                        className="
                          border-b
                          last:border-0
                        "
                      >
                        <td className={CELL}>
                          <span className="font-medium">{t(`band_${row.band}`)}</span>
                          <span className="ml-2 text-xs text-muted-foreground">
                            {row.max_tokens == null
                              ? t('band_over', { tokens: formatInt(150_000) })
                              : t('band_under', { tokens: formatInt(row.max_tokens) })}
                          </span>
                        </td>
                        <td className={HEAD_RIGHT}>{formatInt(row.requests)}</td>
                        <td className={HEAD_RIGHT}>{micros(row.p50_us)}</td>
                        <td className={HEAD_RIGHT}>{micros(row.p95_us)}</td>
                        <td className={cn(HEAD_RIGHT, 'text-muted-foreground')}>
                          {row.body_p50_ms == null ? '—' : duration(row.body_p50_ms)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
        </Card>
      </div>

      {/* ITEM-137: the same bands over time. A band that darkens while its payload
          stays put is a regression; a band whose payload grew is bigger prompts.
          The two are told apart server-side and drawn differently here. */}
      {bands.some(row => row.points.length > 1) && (
        <div className="mt-6">
          <Card title={t('drift_title')} icon="activity">
            <table className="w-full text-sm">
              <thead className="sticky top-0 z-10 bg-card">
                <tr className="border-b text-muted-foreground">
                  <th className={HEAD_LEFT}>{t('band')}</th>
                  {buckets.map(bucket => (
                    <th key={bucket} className={HEAD_RIGHT}>{shortBucket(bucket)}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {bands.map(row => (
                  <tr
                    key={row.band}
                    className="
                      border-b
                      last:border-0
                    "
                  >
                    <td className={CELL}>
                      <span className="font-medium">{t(`band_${row.band}`)}</span>
                    </td>
                    {buckets.map((bucket) => {
                      const point = row.points.find(p => p.bucket === bucket);
                      if (!point) {
                        return (
                          <td
                            key={bucket}
                            className={cn(HEAD_RIGHT, `text-muted-foreground`)}
                          >
                            ·
                          </td>
                        );
                      }
                      return (
                        <td key={bucket} className={cn(HEAD_RIGHT, driftCellClass(point.drift))}>
                          <span>{micros(point.p50_us)}</span>
                          {driftMark(point.drift) && (
                            <span aria-hidden className="ml-1">{driftMark(point.drift)}</span>
                          )}
                          <span className="block text-xs opacity-70">
                            {formatInt(point.prompt_tokens_p50 ?? 0)}
                          </span>
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
            <ul className="
              mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground
            "
            >
              <li>
                {driftMark('regression')}
                {' '}
                {t('drift_regression')}
              </li>
              <li>
                {driftMark('watch')}
                {' '}
                {t('drift_watch')}
              </li>
              <li>
                {driftMark('moved')}
                {' '}
                {t('drift_moved')}
              </li>
              <li>
                ·
                {t('drift_none')}
              </li>
            </ul>
          </Card>
        </div>
      )}

      {/* End to end, so the proxy's share is visible. */}
      <div className="mt-6">
        <Card title={t('end_to_end')} icon="latency">
          <div className="
            grid grid-cols-2 gap-3
            lg:grid-cols-3
          "
          >
            <KpiCard
              label={t('total_p50')}
              icon="latency"
              value={duration(latency.total_p50_ms)}
              window={days}
            />
            <KpiCard
              label={t('total_p95')}
              icon="latency"
              value={duration(latency.total_p95_ms)}
              window={days}
              tone={metricTone(latency.total_p95_ms, THRESHOLDS.totalP95Ms)}
            />
            <KpiCard
              label={t('total_p99')}
              icon="latency"
              value={duration(latency.total_p99_ms)}
              window={days}
            />
            <KpiCard
              label={t('ttft_p50')}
              icon="latency"
              value={duration(latency.ttft_p50_ms)}
              window={days}
            />
            <KpiCard
              label={t('ttft_p95')}
              icon="latency"
              value={duration(latency.ttft_p95_ms)}
              window={days}
              tone={metricTone(latency.ttft_p95_ms, THRESHOLDS.ttftP95Ms)}
            />
            <KpiCard
              label={t('proxy_share')}
              icon="cost"
              value={percent(latency.proxy_share)}
              window={days}
            />
          </div>
        </Card>
      </div>

      {/* Model × time matrix: degradation at a glance (ITEM-099). Empty cells
          are "no traffic" — visually distinct from a fast/healthy day. */}
      {data.heatmap.models.length > 0 && (
        <div className="mt-6">
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
            <div className="overflow-x-auto">
              <table className="w-full text-left text-xs">
                <thead className="sticky top-0 z-10 bg-card">
                  <tr className="border-b text-muted-foreground">
                    <th className="
                      py-2.5 pr-3 font-medium text-muted-foreground
                    "
                    >
                      {t('heatmap_model')}
                    </th>
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

            {/* The tint is a hint, so it has to say what it hints (ITEM-106). */}
            <div className="
              mt-3 flex flex-wrap items-center gap-x-4 gap-y-2 text-xs
              text-muted-foreground
            "
            >
              <span className="flex items-center gap-1.5">
                <span className="inline-block size-3 rounded-sm border" />
                {t('legend_ok')}
              </span>
              <span className="flex items-center gap-1.5">
                <span className="
                  inline-block size-3 rounded-sm bg-destructive/5
                "
                />
                {t('legend_slow')}
              </span>
              <span className="flex items-center gap-1.5">
                <span className="
                  inline-block size-3 rounded-sm bg-destructive/15
                "
                />
                {t('legend_failing')}
              </span>
              <span className="flex items-center gap-1.5">
                <span className="font-mono">·</span>
                {t('legend_empty')}
              </span>
            </div>
          </Card>
        </div>
      )}

      {/* Where the overhead goes. */}
      <div className="mt-6">
        <Card title={t('by_model')} icon="runtime">
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead className="sticky top-0 z-10 bg-card">
                <tr className="border-b">
                  <th className={HEAD}>{t('model')}</th>
                  <th className={HEAD}>{t('requests')}</th>
                  <th className={HEAD}>{t('payload_column')}</th>
                  <th className={HEAD}>{t('upload_column')}</th>
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
                    <td className={CELL}>{formatInt(row.requests)}</td>
                    <td className={CELL}>{formatInt(row.prompt_tokens_p50 ?? 0)}</td>
                    <td className={cn(CELL, 'text-muted-foreground')}>
                      {row.body_p50_ms == null ? '—' : duration(row.body_p50_ms)}
                    </td>
                    <td className={CELL}>{micros(row.proxy_p50_us)}</td>
                    <td className={CELL}>{micros(row.proxy_p95_us)}</td>
                    <td className={CELL}>{duration(row.total_p50_ms)}</td>
                    <td className={CELL}>{duration(row.total_p95_ms)}</td>
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
