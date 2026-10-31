import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { DatasetPurge } from '@/features/dashboard/DatasetPurge';
import { KpiCard } from '@/features/dashboard/KpiCard';
import { PageHeader } from '@/features/shell/PageHeader';
import { formatUsd, InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

const HEAD = 'px-3 py-2 text-left font-mono text-xs uppercase text-muted-foreground';
const CELL = 'px-3 py-2 text-sm tabular-nums';
const DAYS = 30;

const integer = (value: number) => value.toLocaleString('en-US');
const short = (hash: string) => hash.slice(0, 10);

/** The per-cluster JSONL download (owner-only, session cookie). */
const exportHref = (selector: string, model: string, grouping: 'hash' | 'cluster'): string =>
  `/v1/dashboard/api/datasets/export?${new URLSearchParams({
    [grouping === 'cluster' ? 'cluster' : 'prompt_hash']: selector,
    model,
    days: String(DAYS),
  })}`;

/**
 * Datasets (ITEM-086/087): the verified examples production traffic produced,
 * per prompt and model — with the exclusions that did not qualify and a JSONL
 * download. The asset the flywheel is for.
 */
export default async function DatasetsPage(props: {
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({ locale, namespace: 'DatasetsPage' });
  const data = await InferonApi.fromEnv().datasets(DAYS, session.cookie);
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
      <PageHeader title={t('title_bar')} description={t('title_bar_description')} />

      <div className="
        mt-6 grid grid-cols-2 gap-3
        lg:grid-cols-4
      "
      >
        <KpiCard label={t('examples')} value={integer(examples)} hint={t('examples_help')} />
        <KpiCard label={t('clusters')} value={integer(clusters.length)} />
        <KpiCard label={t('spend')} value={formatUsd(spend.toString())} hint={t('spend_help')} />
        <KpiCard label={t('excluded')} value={integer(excluded)} goodWhen="down" />
        <KpiCard label={t('grouping')} value={groupingLabel} hint={t('grouping_help')} />
      </div>

      <div className="mt-4">
        <Card
          title={t('by_prompt')}
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
                      {short(cluster.prompt_hash)}
                    </td>
                    <td className={`
                      ${CELL}
                      font-mono text-xs
                    `}
                    >
                      {cluster.model}
                    </td>
                    <td className={CELL}>{integer(cluster.requests)}</td>
                    <td className={CELL}>{integer(cluster.qualified)}</td>
                    <td className={`
                      ${CELL}
                      font-medium
                    `}
                    >
                      {integer(cluster.distinct_examples)}
                    </td>
                    <td className={CELL}>{integer(cluster.excluded)}</td>
                    <td className="px-3 py-2 text-xs text-muted-foreground">
                      {Object.entries(cluster.excluded_by_reason)
                        .map(([reason, count]) => `${reason}: ${count}`)
                        .join(' · ') || '—'}
                    </td>
                    <td className={CELL}>{formatUsd(cluster.spend_usd)}</td>
                    <td className={`
                      ${CELL}
                      text-xs text-muted-foreground
                    `}
                    >
                      {cluster.last_seen.slice(0, 16)}
                    </td>
                    <td className={CELL}>
                      {cluster.distinct_examples > 0 && (
                        <a
                          className="text-sm underline underline-offset-4"
                          href={exportHref(cluster.prompt_hash, cluster.model, data.grouping)}
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
    </>
  );
}
