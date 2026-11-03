import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { EvalsPanel } from '@/features/dashboard/EvalsPanel';
import { KpiCard } from '@/features/dashboard/KpiCard';
import { reportDays } from '@/features/dashboard/reportWindow';
import { WindowChip } from '@/features/dashboard/WindowChip';
import { PageHeader } from '@/features/shell/PageHeader';
import { InferonApi } from '@/libs/InferonApi';
import { metricTone, THRESHOLDS, toneText } from '@/libs/reportTone';
import { currentSession } from '@/libs/session';

const HEAD = 'px-3 py-2 text-left font-mono text-xs uppercase text-muted-foreground';
const CELL = 'px-3 py-2 text-sm tabular-nums';

const pct = (value: number | null): string =>
  (value == null ? '—' : `${(value * 100).toFixed(1)}%`);

/** The per-model success cell, coloured by the same thresholds as the KPI. */
const successClass = (rate: number | null): string =>
  toneText(metricTone(rate, THRESHOLDS.okRate));

/**
 * Quality view (ITEM-055): how often responses were usable, derived from the
 * response envelope, plus optional client thumbs and the guard-fallback rate.
 */
export default async function QualityPage(props: {
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

  const t = await getTranslations({ locale, namespace: 'QualityPage' });
  const api = InferonApi.fromEnv();
  // ITEM-105: a regression gate is a quality question, so evals live here.
  const [data, evals] = await Promise.all([
    api.quality(30, session.cookie),
    api.evals(session.cookie),
  ]);
  const overall = data.overall;
  const evalsCopy = await getTranslations({ locale, namespace: 'EvalsPage' });

  return (
    <>
      <PageHeader
        title={t('title_bar')}
        description={t('title_bar_description')}
        action={<WindowChip days={days} />}
      />

      <div className="
        mt-6 grid grid-cols-2 gap-3
        lg:grid-cols-3
        xl:grid-cols-6
      "
      >
        <KpiCard
          label={t('success_rate')}
          icon="ok"
          value={pct(overall.success_rate)}
          window={days}
          tone={metricTone(overall.success_rate, THRESHOLDS.okRate)}
          hint={t('success_rate_help')}
        />
        <KpiCard
          label={t('empty_rate')}
          icon="warning"
          value={pct(overall.empty_rate)}
          window={days}
          goodWhen="down"
          definition={t('empty_rate_help')}
        />
        <KpiCard
          label={t('refusal_rate')}
          icon="warning"
          value={pct(overall.refusal_rate)}
          window={days}
          goodWhen="down"
          definition={t('refusal_rate_help')}
        />
        <KpiCard
          label={t('error_rate')}
          icon="warning"
          value={pct(overall.error_rate)}
          window={days}
          goodWhen="down"
          tone={metricTone(overall.error_rate, THRESHOLDS.errorRate)}
          definition={t('error_rate_help')}
        />
        <KpiCard
          label={t('truncated_rate')}
          icon="warning"
          value={pct(overall.truncated_rate)}
          window={days}
          goodWhen="down"
          tone={metricTone(overall.truncated_rate, THRESHOLDS.truncatedRate)}
          definition={t('truncated_rate_help')}
        />
        <KpiCard
          label={t('guard_fallback_rate')}
          icon="warning"
          value={pct(overall.guard_fallback_rate)}
          window={days}
          goodWhen="down"
          definition={t('guard_fallback_rate_help')}
        />
      </div>

      <div className="mt-4">
        <Card title={t('by_error_class')} icon="warning">
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead>
                <tr className="border-b">
                  <th className={HEAD}>{t('class')}</th>
                  <th className={HEAD}>{t('count')}</th>
                </tr>
              </thead>
              <tbody>
                {Object.entries(overall.errors_by_class).map(([errorClass, count]) => (
                  <tr
                    key={errorClass}
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
                      {errorClass}
                    </td>
                    <td className={CELL}>{count}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      </div>

      <div className="mt-4">
        <Card title={t('by_model')} icon="quality">
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead>
                <tr className="border-b">
                  <th className={HEAD}>{t('model')}</th>
                  <th className={HEAD}>{t('requests')}</th>
                  <th className={HEAD}>{t('ok')}</th>
                  <th className={HEAD}>{t('empty')}</th>
                  <th className={HEAD}>{t('errors')}</th>
                  <th className={HEAD}>{t('success')}</th>
                  <th className={HEAD}>{t('thumbs_up')}</th>
                  <th className={HEAD}>{t('thumbs_down')}</th>
                </tr>
              </thead>
              <tbody>
                {data.models.map(model => (
                  <tr
                    key={model.model}
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
                      {model.model}
                    </td>
                    <td className={CELL}>{model.requests}</td>
                    <td className={CELL}>{model.ok}</td>
                    <td className={CELL}>{model.empty}</td>
                    <td className={CELL}>{model.errors}</td>
                    <td className={successClass(model.success_rate)}>
                      {pct(model.success_rate)}
                    </td>
                    <td className={CELL}>{model.thumbs_up}</td>
                    <td className={CELL}>{model.thumbs_down}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      </div>

      <div className="mt-4">
        <Card title={t('by_prompt')} icon="layers">
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead>
                <tr className="border-b">
                  <th className={HEAD}>{t('cluster')}</th>
                  <th className={HEAD}>{t('requests')}</th>
                  <th className={HEAD}>{t('ok')}</th>
                  <th className={HEAD}>{t('empty')}</th>
                  <th className={HEAD}>{t('errors')}</th>
                  <th className={HEAD}>{t('success')}</th>
                </tr>
              </thead>
              <tbody>
                {data.prompts.map(prompt => (
                  <tr
                    key={prompt.system_prompt_hash}
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
                      <span title={prompt.system_prompt_hash}>
                        {prompt.system_prompt_hash.slice(0, 12)}
                      </span>
                    </td>
                    <td className={CELL}>{prompt.requests}</td>
                    <td className={CELL}>{prompt.ok}</td>
                    <td className={CELL}>{prompt.empty}</td>
                    <td className={CELL}>{prompt.errors}</td>
                    <td className={CELL}>{pct(prompt.success_rate)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      </div>
      <div className="mt-4">
        <Card title={evalsCopy('title_bar')} icon="ok">
          <p className="mb-4 text-sm text-muted-foreground">
            {evalsCopy('title_bar_description')}
          </p>
          <EvalsPanel
            goldSets={evals.gold_sets}
            runs={evals.runs}
            canEdit={evals.role === 'owner'}
          />
        </Card>
      </div>
    </>
  );
}
