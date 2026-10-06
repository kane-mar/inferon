import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { StatCard } from '@/features/dashboard/StatCard';
import { WindowSwitcher } from '@/features/dashboard/WindowSwitcher';
import { PageHeader } from '@/features/shell/PageHeader';
import { formatUsd, InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

type OperatorPageProps = {
  params: Promise<{ locale: string }>;
  searchParams: Promise<{ days?: string }>;
};

export default async function OperatorPage(props: OperatorPageProps) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }
  if (!session.me.is_operator) {
    redirect('/dashboard');
  }

  const days = (await props.searchParams).days === '7' ? 7 : 30;
  const t = await getTranslations({
    locale,
    namespace: 'OperatorPage',
  });
  const api = InferonApi.fromEnv();
  const auth = { cookie: session.cookie };
  const [summary, tenants, guard, alerts] = await Promise.all([
    api.operatorSummary(days, auth),
    api.operatorTenants(days, auth),
    api.operatorGuard(days, auth),
    api.operatorAlerts(days, auth),
  ]);

  return (
    <>
      <div className="flex items-start justify-between gap-4">
        <PageHeader
          title={t('title_bar')}
          description={t('title_bar_description', { days })}
        />
        <WindowSwitcher days={days} path="/dashboard/operator" />
      </div>

      <div className="
        grid grid-cols-1 gap-3
        sm:grid-cols-3
      "
      >
        <StatCard label={t('requests')} value={String(summary.requests)} />
        <StatCard label={t('errors')} value={String(summary.errors)} />
        <StatCard label={t('total_cost')} value={formatUsd(summary.total_cost_usd)} />
      </div>

      <div className="
        mt-4 grid grid-cols-1 gap-4
        lg:grid-cols-2
      "
      >
        <Card title={t('tenants')}>
          {tenants.tenants.length === 0
            ? (
                <p className="text-sm text-muted-foreground">{t('no_data')}</p>
              )
            : (
                <div className="overflow-x-auto">
                  <table className="w-full text-left text-sm">
                    <thead>
                      <tr className="border-b text-muted-foreground">
                        <th className="py-2 font-medium">{t('tenant')}</th>
                        <th className="py-2 font-medium">{t('requests')}</th>
                        <th className="py-2 font-medium">{t('errors')}</th>
                        <th className="py-2 font-medium">{t('cost')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {tenants.tenants.map(tenant => (
                        <tr
                          key={tenant.tenant_id}
                          className="
                            border-b
                            last:border-0
                          "
                        >
                          <td className="py-2 font-mono text-xs">{tenant.tenant_id}</td>
                          <td className="py-2 tabular-nums">{tenant.requests}</td>
                          <td className="py-2 tabular-nums">{tenant.errors}</td>
                          <td className="py-2 tabular-nums">{formatUsd(tenant.cost_usd)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
        </Card>

        <Card title={t('guard')}>
          {guard.outcomes.length === 0
            ? (
                <p className="text-sm text-muted-foreground">{t('no_data')}</p>
              )
            : (
                <div className="overflow-x-auto">
                  <table className="w-full text-left text-sm">
                    <thead>
                      <tr className="border-b text-muted-foreground">
                        <th className="py-2 font-medium">{t('outcome')}</th>
                        <th className="py-2 font-medium">{t('requests')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {guard.outcomes.map(outcome => (
                        <tr
                          key={outcome.outcome}
                          className="
                            border-b
                            last:border-0
                          "
                        >
                          <td className="py-2">{outcome.outcome}</td>
                          <td className="py-2 tabular-nums">{outcome.requests}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
        </Card>
      </div>

      <div className="mt-4">
        <Card title={t('alerts')}>
          {alerts.alerts.length === 0
            ? (
                <p className="text-sm text-muted-foreground">{t('no_data')}</p>
              )
            : (
                <div className="overflow-x-auto">
                  <table className="w-full text-left text-sm">
                    <thead>
                      <tr className="border-b text-muted-foreground">
                        <th className="py-2 font-medium">{t('tenant')}</th>
                        <th className="py-2 font-medium">{t('class')}</th>
                        <th className="py-2 font-medium">{t('status_label')}</th>
                        <th className="py-2 font-medium">{t('first_fired')}</th>
                        <th className="py-2 font-medium">{t('last_fired')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {alerts.alerts.map(alert => (
                        <tr
                          key={`${alert.tenant_id}-${alert.class}-${alert.first_fired_at}`}
                          className="
                            border-b
                            last:border-0
                          "
                        >
                          <td className="py-2 font-mono text-xs">{alert.tenant_id}</td>
                          <td className="py-2">{alert.class}</td>
                          <td className="py-2">{alert.status}</td>
                          <td className="py-2">{alert.first_fired_at.slice(0, 16).replace('T', ' ')}</td>
                          <td className="py-2">{alert.last_fired_at.slice(0, 16).replace('T', ' ')}</td>
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
