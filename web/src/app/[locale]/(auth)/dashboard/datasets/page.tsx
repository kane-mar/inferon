// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { DatasetPurge } from '@/features/dashboard/DatasetPurge';
import { KpiCard } from '@/features/dashboard/KpiCard';
import { money } from '@/features/dashboard/money';
import { PromptsPanel } from '@/features/dashboard/PromptsPanel';
import { CELL, HEAD, shortHash } from '@/features/dashboard/reportFormat';
import { reportDays } from '@/features/dashboard/reportWindow';
import { WindowChip } from '@/features/dashboard/WindowChip';
import { PageHeader } from '@/features/shell/PageHeader';
import { formatInt } from '@/libs/dashboardMetrics';
import { dateTime } from '@/libs/datetime';
import { InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

/** The per-cluster JSONL download (owner-only, session cookie). */
const exportHref = (
  selector: string,
  model: string,
  grouping: 'hash' | 'cluster',
  days: number,
): string =>
  `/v1/dashboard/api/datasets/export?${new URLSearchParams({
    [grouping === 'cluster' ? 'cluster' : 'prompt_hash']: selector,
    model,
    days: String(days),
  })}`;

/**
 * Datasets (ITEM-086/087): the verified examples production traffic produced,
 * per prompt and model — with the exclusions that did not qualify and a JSONL
 * download. The asset the flywheel is for.
 */
export default async function DatasetsPage(props: {
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

  const t = await getTranslations({ locale, namespace: 'DatasetsPage' });
  const api = InferonApi.fromEnv();
  // ITEM-105: the registry names the same hashes the clusters group, so it
  // belongs on this page rather than beside the spend reports.
  const [data, prompts] = await Promise.all([
    api.datasets(days, session.cookie),
    api.prompts(session.cookie),
  ]);
  const promptsCopy = await getTranslations({ locale, namespace: 'PromptsPage' });
  const clusters = data.clusters;
  // ITEM-089: say which grouping produced these rows — the exact prompt hash,
  // or the semantic task cluster once the signatures have been computed.
  const groupingLabel = data.grouping === 'cluster' ? t('grouping_cluster') : t('grouping_hash');

  const examples = clusters.reduce((sum, cluster) => sum + cluster.distinct_examples, 0);
  const excluded = clusters.reduce((sum, cluster) => sum + cluster.excluded, 0);
  const spend = clusters.reduce((sum, cluster) => sum + Number(cluster.spend_usd), 0);
  const canExport = data.role === 'owner';

  return (
    <>
      <PageHeader
        title={t('title_bar')}
        action={<WindowChip days={days} />}
      />

      <div className="
        mt-6 grid grid-cols-2 gap-3
        lg:grid-cols-4
      "
      >
        <KpiCard
          label={t('examples')}
          icon="layers"
          value={formatInt(examples)}
          window={days}
        />
        <KpiCard
          label={t('clusters')}
          icon="datasets"
          value={formatInt(clusters.length)}
          window={days}
        />
        <KpiCard
          label={t('spend')}
          icon="cost"
          value={money(spend)}
          window={days}
        />
        <KpiCard
          label={t('excluded')}
          icon="warning"
          value={formatInt(excluded)}
          window={days}
          goodWhen="down"
        />
        <KpiCard label={t('grouping')} icon="layers" value={groupingLabel} />
      </div>

      <div className="mt-4">
        <Card
          title={t('by_prompt')}
          icon="datasets"
          action={canExport ? <DatasetPurge /> : undefined}
        >
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead>
                <tr className="border-b">
                  <th className={HEAD}>{t('prompt')}</th>
                  <th className={HEAD}>{t('model')}</th>
                  <th className={HEAD}>{t('requests')}</th>
                  <th className={HEAD}>{t('qualified')}</th>
                  <th className={HEAD}>{t('distinct')}</th>
                  <th className={HEAD}>{t('excluded')}</th>
                  <th className={HEAD}>{t('why')}</th>
                  <th className={HEAD}>{t('spend')}</th>
                  <th className={HEAD}>{t('last_seen')}</th>
                  <th className={HEAD}>{t('export')}</th>
                </tr>
              </thead>
              <tbody>
                {clusters.map(cluster => (
                  <tr
                    key={`${cluster.prompt_hash}-${cluster.model}`}
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
                      <span title={cluster.prompt_hash}>{shortHash(cluster.prompt_hash)}</span>
                    </td>
                    <td className={`
                      ${CELL}
                      font-mono text-xs
                    `}
                    >
                      {cluster.model}
                    </td>
                    <td className={CELL}>{formatInt(cluster.requests)}</td>
                    <td className={CELL}>{formatInt(cluster.qualified)}</td>
                    <td className={`
                      ${CELL}
                      font-medium
                    `}
                    >
                      {formatInt(cluster.distinct_examples)}
                    </td>
                    <td className={CELL}>{formatInt(cluster.excluded)}</td>
                    <td className="px-3 py-2 text-xs text-muted-foreground">
                      {Object.entries(cluster.excluded_by_reason)
                        .map(([reason, count]) => `${reason}: ${count}`)
                        .join(' · ') || '—'}
                    </td>
                    <td className={CELL}>{money(cluster.spend_usd)}</td>
                    <td className={`
                      ${CELL}
                      text-xs text-muted-foreground
                    `}
                    >
                      <span title={cluster.last_seen}>{dateTime(cluster.last_seen)}</span>
                    </td>
                    <td className={CELL}>
                      {cluster.distinct_examples > 0 && (
                        <a
                          className="text-sm underline underline-offset-4"
                          href={exportHref(cluster.prompt_hash, cluster.model, data.grouping, days)}
                        >
                          {t('export')}
                        </a>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {clusters.length === 0 && (
              <p className="text-sm text-muted-foreground">{t('empty')}</p>
            )}
          </div>
        </Card>
      </div>

      <div className="mt-4">
        <Card title={promptsCopy('title_bar')} icon="layers">
          <PromptsPanel
            prompts={prompts.prompts}
            canEdit={prompts.role === 'owner'}
            contentAvailable={prompts.content_available === true}
          />
        </Card>
      </div>
    </>
  );
}
