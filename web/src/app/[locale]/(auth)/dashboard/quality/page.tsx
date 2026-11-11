// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { EvalsPanel } from '@/features/dashboard/EvalsPanel';
import { KpiCard } from '@/features/dashboard/KpiCard';
import { CELL, HEAD, percent, shortHash } from '@/features/dashboard/reportFormat';
import { reportDays } from '@/features/dashboard/reportWindow';
import { WindowChip } from '@/features/dashboard/WindowChip';
import { PageHeader } from '@/features/shell/PageHeader';
import { InferonApi } from '@/libs/InferonApi';
import { errorClassKeys } from '@/libs/labels';
import { metricTone, THRESHOLDS, toneText } from '@/libs/reportTone';
import { currentSession } from '@/libs/session';

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
  // ITEM-125: a cause is a sentence, not a stored code. The code stays visible as
  // the secondary hint support replies and log lines use.
  const causes = await getTranslations({ locale, namespace: 'ErrorClass' });
  const cause = (code: string) => {
    const key = errorClassKeys(code);
    return { label: key ? causes(key) : code, code: key ? code : null };
  };

  return (
    <>
      <PageHeader
        title={t('title_bar')}
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
          value={percent(overall.success_rate)}
          window={days}
          tone={metricTone(overall.success_rate, THRESHOLDS.okRate)}
        />
        <KpiCard
          label={t('empty_rate')}
          icon="warning"
          value={percent(overall.empty_rate)}
          window={days}
          goodWhen="down"
        />
        <KpiCard
          label={t('refusal_rate')}
          icon="warning"
          value={percent(overall.refusal_rate)}
          window={days}
          goodWhen="down"
        />
        <KpiCard
          label={t('error_rate')}
          icon="warning"
          value={percent(overall.error_rate)}
          window={days}
          goodWhen="down"
          tone={metricTone(overall.error_rate, THRESHOLDS.errorRate)}
        />
        <KpiCard
          label={t('truncated_rate')}
          icon="warning"
          value={percent(overall.truncated_rate)}
          window={days}
          goodWhen="down"
          tone={metricTone(overall.truncated_rate, THRESHOLDS.truncatedRate)}
        />
        <KpiCard
          label={t('guard_fallback_rate')}
          icon="warning"
          value={percent(overall.guard_fallback_rate)}
          window={days}
          goodWhen="down"
        />
      </div>

      <div className="mt-6">
        <Card title={t('by_error_class')} icon="warning">
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead>
                <tr className="border-b">
                  <th className={HEAD}>{t('class')}</th>
                  <th className={`
                    ${HEAD}
                    text-right
                  `}
                  >
                    {t('count')}
                  </th>
                </tr>
              </thead>
              <tbody>
                {Object.entries(overall.errors_by_class).map(([errorClass, count]) => {
                  const { label, code } = cause(errorClass);
                  return (
                    <tr
                      key={errorClass}
                      className="
                        border-b
                        last:border-0
                      "
                    >
                      <td className={CELL}>
                        <div className="font-medium">{label}</div>
                        {code && (
                          <code className="
                            mt-1 block text-xs text-muted-foreground
                          "
                          >
                            {code}
                          </code>
                        )}
                      </td>
                      <td className={`
                        ${CELL}
                        text-right
                      `}
                      >
                        {count}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </Card>
      </div>

      <div className="mt-6">
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
                      {percent(model.success_rate)}
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

      <div className="mt-6">
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
                        {shortHash(prompt.system_prompt_hash, 12)}
                      </span>
                    </td>
                    <td className={CELL}>{prompt.requests}</td>
                    <td className={CELL}>{prompt.ok}</td>
                    <td className={CELL}>{prompt.empty}</td>
                    <td className={CELL}>{prompt.errors}</td>
                    <td className={CELL}>{percent(prompt.success_rate)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      </div>
      <div className="mt-6">
        <Card title={evalsCopy('title_bar')} icon="ok">
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
