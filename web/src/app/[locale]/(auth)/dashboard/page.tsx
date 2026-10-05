import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { StatCard } from '@/features/dashboard/StatCard';
import { TitleBar } from '@/features/dashboard/TitleBar';
import { WindowSwitcher } from '@/features/dashboard/WindowSwitcher';
import { formatUsd, InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

type DashboardIndexPageProps = {
  params: Promise<{ locale: string }>;
  searchParams: Promise<{ days?: string }>;
};

export default async function DashboardIndexPage(props: DashboardIndexPageProps) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const days = (await props.searchParams).days === '7' ? 7 : 30;
  const t = await getTranslations({
    locale,
    namespace: 'DashboardIndexPage',
  });
  const api = InferonApi.fromEnv();
  const [summary, byModel, byPrompt, waste] = await Promise.all([
    api.summary(days, session.cookie),
    api.byModel(days, session.cookie),
    api.byPrompt(days, 10, session.cookie),
    api.waste(days, session.cookie),
  ]);
  const wasteRows = Object.entries(waste);
  const wasteLabels: Record<string, string> = {
    over_provisioned: t('waste_over_provisioned'),
    cache_miss: t('waste_cache_miss'),
    model_misuse: t('waste_model_misuse'),
  };

  return (
    <>
      <div className="flex items-start justify-between gap-4">
        <TitleBar
          title={t('title_bar')}
          description={t('title_bar_description', { days })}
        />
        <WindowSwitcher days={days} path="/dashboard" />
      </div>

      <div className="
        grid grid-cols-1 gap-3
        sm:grid-cols-2
        lg:grid-cols-4
      "
      >
        <StatCard label={t('requests')} value={String(summary.requests)} />
        <StatCard label={t('errors')} value={String(summary.errors)} />
        <StatCard
          label={t('total_cost')}
          value={formatUsd(summary.total_cost_usd)}
        />
        <StatCard
          label={t('last_trace')}
          value={summary.last_trace_at?.slice(0, 16).replace('T', ' ') ?? '—'}
          hint={
            summary.first_trace_at
              ? t('since', { date: summary.first_trace_at.slice(0, 10) })
              : undefined
          }
        />
      </div>

      <div className="
        mt-4 grid grid-cols-1 gap-4
        lg:grid-cols-2
      "
      >
        <Card title={t('by_model')}>
          {byModel.models.length === 0
            ? (
                <p className="text-sm text-muted-foreground">{t('no_data')}</p>
              )
            : (
                <div className="overflow-x-auto">
                  <table className="w-full text-left text-sm">
                    <thead>
                      <tr className="border-b text-muted-foreground">
                        <th className="py-2 font-medium">{t('model')}</th>
                        <th className="py-2 font-medium">{t('requests')}</th>
                        <th className="py-2 font-medium">{t('prompt_tokens')}</th>
                        <th className="py-2 font-medium">{t('completion_tokens')}</th>
                        <th className="py-2 font-medium">{t('cost')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {byModel.models.map(model => (
                        <tr
                          key={model.model}
                          className="
                            border-b
                            last:border-0
                          "
                        >
                          <td className="py-2">{model.model}</td>
                          <td className="py-2 tabular-nums">{model.requests}</td>
                          <td className="py-2 tabular-nums">{model.prompt_tokens}</td>
                          <td className="py-2 tabular-nums">{model.completion_tokens}</td>
                          <td className="py-2 tabular-nums">{formatUsd(model.cost_usd)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
        </Card>

        <Card title={t('by_prompt')}>
          {byPrompt.prompts.length === 0
            ? (
                <p className="text-sm text-muted-foreground">{t('no_data')}</p>
              )
            : (
                <div className="overflow-x-auto">
                  <table className="w-full text-left text-sm">
                    <thead>
                      <tr className="border-b text-muted-foreground">
                        <th className="py-2 font-medium">{t('prompt_hash')}</th>
                        <th className="py-2 font-medium">{t('requests')}</th>
                        <th className="py-2 font-medium">{t('prompt_tokens')}</th>
                        <th className="py-2 font-medium">{t('cost')}</th>
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
                          <td className="py-2 tabular-nums">{prompt.requests}</td>
                          <td className="py-2 tabular-nums">{prompt.prompt_tokens}</td>
                          <td className="py-2 tabular-nums">{formatUsd(prompt.cost_usd)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
        </Card>
      </div>

      <div className="mt-4">
        <Card title={t('waste')}>
          {wasteRows.length === 0
            ? (
                <p className="text-sm text-muted-foreground">{t('no_data')}</p>
              )
            : (
                <div className="overflow-x-auto">
                  <table className="w-full text-left text-sm">
                    <thead>
                      <tr className="border-b text-muted-foreground">
                        <th className="py-2 font-medium">{t('waste_class')}</th>
                        <th className="py-2 font-medium">{t('requests')}</th>
                        <th className="py-2 font-medium">{t('spend')}</th>
                        <th className="py-2 font-medium">{t('estimated_savings')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {wasteRows.map(([flag, row]) => (
                        <tr
                          key={flag}
                          className="
                            border-b
                            last:border-0
                          "
                        >
                          <td className="py-2">
                            {wasteLabels[flag] ?? flag}
                          </td>
                          <td className="py-2 tabular-nums">{row.requests}</td>
                          <td className="py-2 tabular-nums">{formatUsd(row.spend_usd)}</td>
                          <td className="py-2 tabular-nums">
                            {formatUsd(row.savings_est_usd)}
                          </td>
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
