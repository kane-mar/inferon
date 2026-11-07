// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import type { OperatorGuard } from '@/libs/InferonApi';
import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { KpiCard } from '@/features/dashboard/KpiCard';
import { money } from '@/features/dashboard/money';
import { WindowChip } from '@/features/dashboard/WindowChip';
import { PageHeader } from '@/features/shell/PageHeader';
import { InferonApi } from '@/libs/InferonApi';
import { guardOutcomeKeys } from '@/libs/labels';
import { currentSession } from '@/libs/session';

/**
 * Guard outcomes from the aggregate: flushed = guarded minus the two fallback
 * outcomes (never negative).
 */
const guardOutcomes = (guard: OperatorGuard) => [
  {
    outcome: 'flushed',
    requests: Math.max(
      0,
      guard.guarded_requests - guard.fallback_requests - guard.fallback_failed,
    ),
  },
  { outcome: 'fallback', requests: guard.fallback_requests },
  { outcome: 'fallback_failed', requests: guard.fallback_failed },
];

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
  // ITEM-125: an outcome is a sentence. The stored value stays as the hint.
  const outcomes = await getTranslations({ locale, namespace: 'Outcome' });
  const api = InferonApi.fromEnv();
  const auth = { cookie: session.cookie };
  const [summary, tenants, guard, alerts, activation, evolution] = await Promise.all([
    api.operatorSummary(days, auth),
    api.operatorTenants(days, auth),
    api.operatorGuard(days, auth),
    api.operatorAlerts(days, auth),
    api.operatorActivation(days, auth),
    api.operatorEvolution(auth),
  ]);

  return (
    <>
      <div className="flex items-start justify-between gap-4">
        <PageHeader
          title={t('title_bar')}
          description={t('title_bar_description', { days })}
        />
        <WindowChip days={days} />
      </div>

      <div className="
        grid grid-cols-1 gap-3
        sm:grid-cols-3
      "
      >
        <KpiCard label={t('requests')} icon="activity" value={String(summary.requests)} unit={t('requests_unit')} window={days} definition={t('requests_help')} />
        <KpiCard label={t('errors')} icon="warning" value={String(summary.errors)} unit={t('requests_unit')} window={days} definition={t('errors_help')} />
        <KpiCard label={t('total_cost')} icon="cost" value={money(summary.total_cost_usd)} window={days} definition={t('total_cost_help')} />
      </div>

      <div className="
        mt-4 grid grid-cols-1 gap-4
        lg:grid-cols-2
      "
      >
        <Card title={t('engagement')} icon="activity">
          <div className="
            grid grid-cols-2 gap-3
            sm:grid-cols-4
          "
          >
            <KpiCard label={t('active_tenants')} icon="store" value={String(activation.active_tenants)} />
            <KpiCard label={t('active_keys')} icon="lock" value={String(activation.active_keys)} />
            <KpiCard
              label={t('requests_per_dev')}
              icon="activity"
              value={
                activation.requests_per_active_tenant == null
                  ? '—'
                  : activation.requests_per_active_tenant.toFixed(1)
              }
            />
            <KpiCard label={t('signups')} icon="up" value={String(activation.signups)} />
          </div>
        </Card>

        <Card title={t('funnel')} icon="layers">
          <ul className="text-sm">
            {([
              [t('signed_up'), activation.funnel.signed_up],
              [t('created_provider'), activation.funnel.created_provider],
              [t('first_request'), activation.funnel.first_request],
              [t('returned'), activation.funnel.returned],
            ] as const).map(([label, count]) => (
              <li
                key={label}
                className="
                  flex justify-between border-b py-2
                  last:border-0
                "
              >
                <span className="text-muted-foreground">{label}</span>
                <span className="tabular-nums">{count}</span>
              </li>
            ))}
          </ul>
        </Card>
      </div>

      <div className="
        mt-4 grid grid-cols-1 gap-4
        lg:grid-cols-2
      "
      >
        <Card title={t('tenants')} icon="store">
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
                          <td className="py-2 tabular-nums">{money(tenant.cost_usd)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
        </Card>

        <Card title={t('guard')} icon="quality">
          {guard.guarded_requests === 0
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
                      {guardOutcomes(guard).map(outcome => (
                        <tr
                          key={outcome.outcome}
                          className="
                            border-b
                            last:border-0
                          "
                        >
                          <td className="py-2">
                            {(() => {
                              const keys = guardOutcomeKeys(outcome.outcome);
                              if (!keys) {
                                return outcome.outcome;
                              }
                              return (
                                <div>
                                  <div>{outcomes(keys.label)}</div>
                                  <code className="text-xs text-muted-foreground">
                                    {outcome.outcome}
                                  </code>
                                </div>
                              );
                            })()}
                          </td>
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
        <Card title={t('alerts')} icon="warning">
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
                          <td className="py-2">
                            {alert.provider_name ?? alert.class}
                          </td>
                          <td className="py-2">{alert.status}</td>
                          <td className="py-2">
                            <span title={alert.first_fired_at}>
                              {alert.first_fired_at.slice(0, 16).replace('T', ' ')}
                            </span>
                          </td>
                          <td className="py-2">
                            <span title={alert.last_fired_at}>
                              {alert.last_fired_at.slice(0, 16).replace('T', ' ')}
                            </span>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
        </Card>

        <Card title={t('evolution')} icon="layers">
          {evolution.jobs.length === 0
            ? (
                <p className="text-sm text-muted-foreground">{t('no_data')}</p>
              )
            : (
                <div className="overflow-x-auto">
                  <table className="w-full text-left text-sm">
                    <thead>
                      <tr className="border-b text-muted-foreground">
                        <th className="py-2 font-medium">{t('tenant')}</th>
                        <th className="py-2 font-medium">{t('adapter')}</th>
                        <th className="py-2 font-medium">{t('state')}</th>
                        <th className="py-2 font-medium">{t('fallback_rate')}</th>
                        <th className="py-2 font-medium">{t('detected_at')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {evolution.jobs.map(job => (
                        <tr
                          key={job.id}
                          className="
                            border-b
                            last:border-0
                          "
                        >
                          <td className="py-2 font-mono text-xs">{job.tenant_id}</td>
                          <td className="py-2">{job.adapter}</td>
                          <td className="py-2">{job.state}</td>
                          <td className="py-2">
                            {job.evidence.rate === undefined
                              ? '—'
                              : `${(job.evidence.rate * 100).toFixed(1)}%`}
                          </td>
                          <td className="py-2">
                            <span title={job.created_at}>
                              {job.created_at.slice(0, 16).replace('T', ' ')}
                            </span>
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
