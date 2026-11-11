// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { Empty } from '@/features/dashboard/Empty';
import { EvalsPanel } from '@/features/dashboard/EvalsPanel';
import { KpiCard } from '@/features/dashboard/KpiCard';
import { OutcomeBar, ShareBar } from '@/features/dashboard/OutcomeBar';
import { CELL, HEAD_LEFT, HEAD_RIGHT, percent, shortHash } from '@/features/dashboard/reportFormat';
import { reportDays } from '@/features/dashboard/reportWindow';
import { WindowChip } from '@/features/dashboard/WindowChip';
import { PageHeader } from '@/features/shell/PageHeader';
import { formatInt } from '@/libs/dashboardMetrics';
import { InferonApi } from '@/libs/InferonApi';
import { errorClassKeys } from '@/libs/labels';
import { metricTone, THRESHOLDS, toneText } from '@/libs/reportTone';
import { share, split } from '@/libs/segments';
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

  // ITEM-148: what the requests were, in the order a reader thinks about them.
  // `unclassified` is traffic the envelope could not name, so it is shown rather
  // than dropped: a bar that hides a slice is worse than one that is ugly.
  const outcomeSegments = split([
    { key: 'ok', count: overall.ok },
    { key: 'empty', count: overall.empty },
    { key: 'refused', count: overall.refused },
    { key: 'truncated', count: overall.truncated },
    { key: 'error', count: overall.errors },
    { key: 'other', count: overall.unclassified },
  ]);
  const outcomeLabels: Record<string, string> = {
    ok: t('success_rate'),
    empty: t('empty_rate'),
    refused: t('refusal_rate'),
    truncated: t('truncated_rate'),
    error: t('error_rate'),
    other: t('other'),
  };

  return (
    <>
      <PageHeader
        title={t('title_bar')}
        action={<WindowChip days={days} />}
      />

      {/* ITEM-148: the page opened with six equal boxes, five of them failure
          rates, so it read as six alarms. What the requests actually were belongs
          in one bar; the numbers that can breach a threshold stay as cards. */}
      <div className="
        mt-6 grid grid-cols-2 gap-3
        lg:grid-cols-4
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
          label={t('requests')}
          icon="activity"
          value={formatInt(overall.requests)}
          window={days}
        />
      </div>

      <div className="mt-6">
        <Card title={t('outcomes')} icon="quality">
          <OutcomeBar segments={outcomeSegments} labels={outcomeLabels} />
        </Card>
      </div>

      {/* Two questions side by side, each a short list: why requests failed, and
          which prompt groups they came from. Full-width cards for four rows was
          most of this page's height. */}
      <div className="
        mt-6 grid grid-cols-1 gap-6
        lg:grid-cols-2
      "
      >
        <Card title={t('by_error_class')} icon="warning">
          {Object.keys(overall.errors_by_class).length === 0
            ? <Empty label={t('no_errors')} />
            : (
                <table className="w-full">
                  <thead>
                    <tr className="border-b text-muted-foreground">
                      <th className={HEAD_LEFT}>{t('class')}</th>
                      <th className={HEAD_RIGHT}>{t('count')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {Object.entries(overall.errors_by_class).map(([errorClass, count]) => {
                      const named = cause(errorClass);
                      return (
                        <tr
                          key={errorClass}
                          className="
                            border-b
                            last:border-0
                          "
                        >
                          <td className={CELL}>
                            <div className="font-medium">{named.label}</div>
                            {named.code && (
                              <code className="
                                mt-1 block text-xs text-muted-foreground
                              "
                              >
                                {named.code}
                              </code>
                            )}
                          </td>
                          <td className={`
                            ${CELL}
                            text-right
                          `}
                          >
                            <span className="tabular-nums">{formatInt(count)}</span>
                            <ShareBar share={share(count, overall.requests)} tone="bad" />
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              )}
        </Card>

        <Card title={t('by_prompt')} icon="layers">
          {data.prompts.length === 0
            ? <Empty label={t('no_data')} />
            : (
                <table className="w-full">
                  <thead>
                    <tr className="border-b text-muted-foreground">
                      <th className={HEAD_LEFT}>{t('cluster')}</th>
                      <th className={HEAD_RIGHT}>{t('requests')}</th>
                      <th className={HEAD_RIGHT}>{t('success')}</th>
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
                        <td className={`
                          ${CELL}
                          text-right
                        `}
                        >
                          {formatInt(prompt.requests)}
                        </td>
                        <td className={`
                          ${CELL}
                          text-right
                          ${successClass(prompt.success_rate)}
                        `}
                        >
                          {percent(prompt.success_rate)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
        </Card>
      </div>

      <div className="mt-6">
        <Card title={t('by_model')} icon="quality">
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead>
                <tr className="border-b text-muted-foreground">
                  <th className={HEAD_LEFT}>{t('model')}</th>
                  <th className={HEAD_RIGHT}>{t('requests')}</th>
                  <th className={HEAD_RIGHT}>{t('ok')}</th>
                  <th className={HEAD_RIGHT}>{t('empty')}</th>
                  <th className={HEAD_RIGHT}>{t('errors')}</th>
                  <th className={HEAD_RIGHT}>{t('success')}</th>
                  <th className={HEAD_RIGHT}>{t('thumbs_up')}</th>
                  <th className={HEAD_RIGHT}>{t('thumbs_down')}</th>
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
                    <td className={`
                      ${CELL}
                      text-right
                    `}
                    >
                      {formatInt(model.requests)}
                    </td>
                    <td className={`
                      ${CELL}
                      text-right
                    `}
                    >
                      {formatInt(model.ok)}
                    </td>
                    <td className={`
                      ${CELL}
                      text-right
                    `}
                    >
                      {formatInt(model.empty)}
                    </td>
                    <td className={`
                      ${CELL}
                      text-right
                    `}
                    >
                      {formatInt(model.errors)}
                    </td>
                    <td className={`
                      ${CELL}
                      text-right
                      ${successClass(model.success_rate)}
                    `}
                    >
                      {percent(model.success_rate)}
                    </td>
                    <td className={`
                      ${CELL}
                      text-right
                    `}
                    >
                      {formatInt(model.thumbs_up)}
                    </td>
                    <td className={`
                      ${CELL}
                      text-right
                    `}
                    >
                      {formatInt(model.thumbs_down)}
                    </td>
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
