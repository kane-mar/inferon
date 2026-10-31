import type { SpendBreakdownRow } from '@/features/dashboard/SpendBreakdown';
import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Suspense } from 'react';
import { BarChart } from '@/features/dashboard/BarChart';
import { Card } from '@/features/dashboard/Card';
import { HideZeroCheckbox } from '@/features/dashboard/HideZeroCheckbox';
import { SpendBreakdown } from '@/features/dashboard/SpendBreakdown';
import { PageHeader } from '@/features/shell/PageHeader';
import { formatCompact, formatInt, spendRows } from '@/libs/dashboardMetrics';
import { Link } from '@/libs/I18nNavigation';
import { formatUsd, InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

const WINDOWS = [7, 30, 90] as const;

const pill = (active: boolean): string =>
  active
    ? 'rounded-[5px] bg-primary px-2.5 py-1 font-medium text-primary-foreground'
    : `
      rounded-[5px] px-2.5 py-1 text-muted-foreground
      hover:bg-muted hover:text-foreground
    `;

const HEAD = 'py-2 font-mono text-xs font-medium tracking-wide uppercase';
const RIGHT = `${HEAD} text-right`;

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
  const requested = Number(params.days);
  const days = (WINDOWS as readonly number[]).includes(requested) ? requested : 30;
  const hideZero = params.hide_zero === '1';
  const t = await getTranslations({ locale, namespace: 'DashboardIndexPage' });
  const heading = await getTranslations({ locale, namespace: 'SpendReportPage' });
  const api = InferonApi.fromEnv();

  const [byModel, byProvider, byKey, byPrompt, keys] = await Promise.all([
    api.byModel(days, session.cookie),
    api.byProvider(days, 0, session.cookie),
    api.byKey(days, 0, session.cookie),
    api.byPrompt(days, 10, session.cookie),
    api.keys(session.cookie),
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
      <div className="flex flex-wrap items-start justify-between gap-4">
        <PageHeader
          title={heading('title_bar')}
          description={heading('title_bar_description', { days })}
        />
        <div className="flex items-center gap-3">
          <Suspense fallback={null}>
            <HideZeroCheckbox checked={hideZero} label={t('hide_zero')} />
          </Suspense>
          <div className="
            inline-flex items-center gap-0.5 rounded-md border bg-card p-0.5
            text-sm shadow-sm
          "
          >
            {WINDOWS.map(window => (
              <Link
                key={window}
                href={{
                  pathname: '/dashboard/reports/spend',
                  query: { days: window, ...(hideZero ? { hide_zero: 1 } : {}) },
                }}
                className={pill(window === days)}
              >
                {`${window}d`}
              </Link>
            ))}
          </div>
        </div>
      </div>

      <div className="
        mt-6 grid grid-cols-1 gap-4
        lg:grid-cols-2
      "
      >
        <Card title={t('by_model')}>
          <BarChart
            rows={models.map(model => ({
              label: model.model,
              value: Number(model.cost_usd),
              display: formatUsd(model.cost_usd),
            }))}
            empty={t('no_data')}
          />
          {models.length > 0 && (
            <table className="mt-4 w-full text-left text-sm">
              <thead>
                <tr className="border-b text-muted-foreground">
                  <th className={HEAD}>{t('model')}</th>
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
                    <td className="py-2 text-right tabular-nums">{formatUsd(model.cost_usd)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Card>

        <Card title={t('by_provider')}>
          <BarChart
            rows={providers.map(provider => ({
              label: provider.name,
              value: Number(provider.cost_usd),
              display: formatUsd(provider.cost_usd),
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

        <Card title={t('by_key')}>
          <BarChart
            rows={keySource.map(key => ({
              label: key.label,
              value: Number(key.cost_usd),
              display: formatUsd(key.cost_usd),
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

        <Card title={t('by_prompt')}>
          {byPrompt.prompts.length === 0
            ? (
                <p className="text-sm text-muted-foreground">{t('no_data')}</p>
              )
            : (
                <table className="w-full text-left text-sm">
                  <thead>
                    <tr className="border-b text-muted-foreground">
                      <th className={HEAD}>{t('prompt_hash')}</th>
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
                          {formatUsd(prompt.cost_usd)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
        </Card>
      </div>
    </>
  );
}
