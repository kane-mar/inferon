// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import type { SpendBreakdownRow } from '@/features/dashboard/SpendBreakdown';
import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Suspense } from 'react';
import { BarChart } from '@/features/dashboard/BarChart';
import { Card } from '@/features/dashboard/Card';
import { HideZeroCheckbox } from '@/features/dashboard/HideZeroCheckbox';
import { KpiCard } from '@/features/dashboard/KpiCard';
import { money } from '@/features/dashboard/money';
import { CELL, HEAD as REPORT_HEAD } from '@/features/dashboard/reportFormat';
import { reportDays } from '@/features/dashboard/reportWindow';
import { SpendBreakdown } from '@/features/dashboard/SpendBreakdown';
import { WindowChip } from '@/features/dashboard/WindowChip';
import { PageHeader } from '@/features/shell/PageHeader';
import { formatCompact, formatInt, spendRows } from '@/libs/dashboardMetrics';
import { InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

// ITEM-124: one header atom for every report — 13px sans, sentence case.
const RIGHT = `${REPORT_HEAD} text-right`;

/** The spend detail: every dollar by model, provider, key and system prompt. */
export default async function SpendReportPage(props: {
  params: Promise<{ locale: string }>;
  searchParams: Promise<{ days?: string; hide_zero?: string }>;
}) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const params = await props.searchParams;
  const days = reportDays(params);
  const hideZero = params.hide_zero === '1';
  const t = await getTranslations({ locale, namespace: 'DashboardIndexPage' });
  const heading = await getTranslations({ locale, namespace: 'SpendReportPage' });
  const api = InferonApi.fromEnv();

  // ITEM-103: the unit of work — what one client transaction costs — is a spend
  // question, so it lives here rather than beside the latency numbers.
  const [byModel, byProvider, byKey, byPrompt, keys, sessions] = await Promise.all([
    api.byModel(days, session.cookie),
    api.byProvider(days, 0, session.cookie),
    api.byKey(days, 0, session.cookie),
    api.byPrompt(days, 10, session.cookie),
    api.keys(session.cookie),
    api.sessions(days, session.cookie),
  ]);

  const models = spendRows(byModel.models, hideZero);
  const providers = spendRows(byProvider.providers, hideZero);
  const keySource = spendRows(byKey.keys, hideZero);
  const canEdit = keys.role === 'owner';

  const providerRows: SpendBreakdownRow[] = providers.map(provider => ({
    id: provider.provider_id,
    label: provider.name,
    meta: provider.kind === 'shared' ? undefined : provider.kind,
    costUsd: provider.cost_usd,
    previousCostUsd: null,
    requests: provider.requests,
    tokens: provider.prompt_tokens + provider.completion_tokens,
    limitCents: provider.spend_limit_cents,
    errorRate: provider.error_rate,
    p95Ms: provider.ttft_p95_ms,
  }));

  const keyRows: SpendBreakdownRow[] = keySource.map(key => ({
    id: key.key_id,
    label: key.label,
    costUsd: key.cost_usd,
    previousCostUsd: null,
    requests: key.requests,
    tokens: key.prompt_tokens + key.completion_tokens,
    limitCents: key.spend_limit_cents,
    rpmLimit: key.rpm_limit,
    tpmLimit: key.tpm_limit,
  }));

  return (
    <>
      <PageHeader
        title={heading('title_bar')}
        description={heading('title_bar_description', { days })}
        action={(
          <div className="flex items-center gap-3">
            <Suspense fallback={null}>
              <HideZeroCheckbox checked={hideZero} label={t('hide_zero')} />
            </Suspense>
            <WindowChip days={days} />
          </div>
        )}
      />

      <div className="
        mt-6 grid grid-cols-1 gap-4
        lg:grid-cols-2
      "
      >
        <Card title={t('by_model')} icon="cost">
          <BarChart
            rows={models.map(model => ({
              label: model.model,
              value: Number(model.cost_usd),
              display: money(model.cost_usd),
            }))}
            empty={t('no_data')}
          />
          {models.length > 0 && (
            <table className="mt-4 w-full text-left text-sm">
              <thead>
                <tr className="border-b text-muted-foreground">
                  <th className={REPORT_HEAD}>{t('model')}</th>
                  <th className={RIGHT}>{t('requests')}</th>
                  <th className={RIGHT}>{t('tokens')}</th>
                  <th className={RIGHT}>{t('cached')}</th>
                  <th className={RIGHT}>{t('cost')}</th>
                </tr>
              </thead>
              <tbody>
                {models.map(model => (
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
                    <td className="py-2 text-right tabular-nums">{money(model.cost_usd)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Card>

        <Card title={t('by_provider')} icon="store">
          <BarChart
            rows={providers.map(provider => ({
              label: provider.name,
              value: Number(provider.cost_usd),
              display: money(provider.cost_usd),
            }))}
            empty={t('no_data')}
          />
          {providerRows.length > 0 && (
            <div className="mt-4">
              <SpendBreakdown
                kind="provider"
                rows={providerRows}
                canEdit={canEdit}
                emptyLabel={t('no_data')}
              />
            </div>
          )}
        </Card>

        <Card title={t('by_key')} icon="lock">
          <BarChart
            rows={keySource.map(key => ({
              label: key.label,
              value: Number(key.cost_usd),
              display: money(key.cost_usd),
            }))}
            empty={t('no_data')}
          />
          {keyRows.length > 0 && (
            <div className="mt-4">
              <SpendBreakdown
                kind="key"
                rows={keyRows}
                canEdit={canEdit}
                emptyLabel={t('no_data')}
              />
            </div>
          )}
        </Card>

        <Card title={t('by_prompt')} icon="layers">
          {byPrompt.prompts.length === 0
            ? (
                <p className="text-sm text-muted-foreground">{t('no_data')}</p>
              )
            : (
                <table className="w-full text-left text-sm">
                  <thead>
                    <tr className="border-b text-muted-foreground">
                      <th className={REPORT_HEAD}>{t('prompt_hash')}</th>
                      <th className={RIGHT}>{t('requests')}</th>
                      <th className={RIGHT}>{t('prompt_tokens')}</th>
                      <th className={RIGHT}>{t('cached')}</th>
                      <th className={RIGHT}>{t('cost')}</th>
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
                        <td className="
                          py-2 text-right text-muted-foreground tabular-nums
                        "
                        >
                          {prompt.prompt_tokens > 0
                            ? `${Math.round((prompt.cached_prompt_tokens / prompt.prompt_tokens) * 100)}%`
                            : '—'}
                        </td>
                        <td className="py-2 text-right tabular-nums">
                          {money(prompt.cost_usd)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
        </Card>
      </div>

      {/* The unit of work: what one client transaction actually costs. */}
      <div className="mt-4">
        <Card title={heading('transactions')} icon="activity">
          <div className="grid grid-cols-3 gap-3">
            <KpiCard
              label={heading('transactions_p50')}
              icon="latency"
              value={money(sessions.distribution.p50_cost_usd)}
              window={days}
              definition={heading('transactions_help')}
            />
            <KpiCard
              label={heading('transactions_p95')}
              icon="latency"
              value={money(sessions.distribution.p95_cost_usd)}
              window={days}
              definition={heading('transactions_help')}
            />
            <KpiCard
              label={heading('transactions_count')}
              icon="activity"
              value={formatInt(sessions.distribution.sessions)}
              window={days}
              definition={heading('transactions_help')}
            />
          </div>
          <p className="mt-3 text-xs text-muted-foreground">{heading('transactions_help')}</p>
          {sessions.sessions.length > 0 && (
            <div className="mt-3 overflow-x-auto">
              <table className="w-full">
                <thead>
                  <tr className="border-b">
                    <th className={REPORT_HEAD}>{heading('transaction')}</th>
                    <th className={REPORT_HEAD}>{heading('turns')}</th>
                    <th className={REPORT_HEAD}>{heading('models')}</th>
                    <th className={REPORT_HEAD}>{heading('cost')}</th>
                    <th className={REPORT_HEAD}>{heading('errors')}</th>
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
                      <td className={CELL}>{formatInt(row.turns)}</td>
                      <td className={CELL}>{formatInt(row.models)}</td>
                      <td className={CELL}>{money(row.cost_usd)}</td>
                      <td className={CELL}>{formatInt(row.errors)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Card>
      </div>
    </>
  );
}
