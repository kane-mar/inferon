// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import type { AuditData } from '@/features/dashboard/AuditCards';
import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { AuditCards } from '@/features/dashboard/AuditCards';
import { AutoRefresh } from '@/features/dashboard/AutoRefresh';
import { Card } from '@/features/dashboard/Card';
import { reportDays } from '@/features/dashboard/reportWindow';
import { ShareAuditPanel } from '@/features/dashboard/ShareAuditPanel';
import { WindowChip } from '@/features/dashboard/WindowChip';
import { PageHeader } from '@/features/shell/PageHeader';
import { Link } from '@/libs/I18nNavigation';
import { InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

/**
 * The cost audit: six named findings computed from the tenant's own telemetry
 * (guides/inferon-cost-audit.md), refreshed server-side every 15 seconds.
 */
export default async function AuditPage(props: {
  params: Promise<{ locale: string }>;
  searchParams: Promise<{ days?: string }>;
}) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const days = reportDays(await props.searchParams);
  const t = await getTranslations({ locale, namespace: 'AuditPage' });
  const api = InferonApi.fromEnv();

  const [summary, waste, quality, byModel, byProvider, byKey, keys, shares]
    = await Promise.all([
      api.summary(days, 0, session.cookie),
      api.waste(days, session.cookie),
      api.quality(days, session.cookie),
      api.byModel(days, session.cookie),
      api.byProvider(days, 0, session.cookie),
      api.byKey(days, 0, session.cookie),
      api.keys(session.cookie),
      api.auditShares(session.cookie),
    ]);

  const overall = quality.overall;
  const topModel = byModel.models[0];
  const topProvider = byProvider.providers[0];
  const topKey = byKey.keys[0];
  const data: AuditData = {
    requests: summary.requests,
    cost: summary.total_cost_usd,
    savings: {
      total: summary.total_savings_usd,
      router: summary.router_savings_usd,
      fallback: summary.fallback_savings_usd,
      cache: summary.cache_savings_usd,
      affinity: summary.cache_affinity_requests,
    },
    waste,
    top: {
      model: topModel ? { name: topModel.model, cost: topModel.cost_usd } : undefined,
      provider: topProvider
        ? { name: topProvider.name, cost: topProvider.cost_usd }
        : undefined,
      key: topKey ? { name: topKey.label, cost: topKey.cost_usd } : undefined,
    },
    quality: {
      errorRate: overall.error_rate,
      truncatedRate: overall.truncated_rate,
      guardFallbackRate: overall.guard_fallback_rate,
      topFailure: Object.entries(overall.errors_by_class).sort((a, b) => b[1] - a[1])[0],
    },
  };

  return (
    <>
      <PageHeader
        title={t('title_bar')}
        action={(
          <div className="flex items-center gap-3">
            <AutoRefresh label={t('live')} />
            <WindowChip days={days} />
          </div>
        )}
      />

      {summary.requests === 0 && (
        <div className="mb-4">
          <Card title={t('no_traffic_title')} icon="warning">
            <p className="text-sm text-muted-foreground">{t('no_traffic_body')}</p>
            <Link
              href="/dashboard/settings"
              className="
                mt-3 inline-block text-sm font-medium text-primary underline
                underline-offset-4
              "
            >
              {t('no_traffic_cta')}
            </Link>
          </Card>
        </div>
      )}

      <AuditCards locale={locale} data={data} />

      {keys.role === 'owner' && (
        <div className="mt-4">
          <Card title={t('share_title')} icon="lock">
            <p className="mb-3 text-sm text-muted-foreground">{t('share_body')}</p>
            <ShareAuditPanel data={shares} />
          </Card>
        </div>
      )}
    </>
  );
}
