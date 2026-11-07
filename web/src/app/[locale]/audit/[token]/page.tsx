// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import type { AuditData } from '@/features/dashboard/AuditCards';
import { getTranslations, setRequestLocale } from 'next-intl/server';
import { AuditCards } from '@/features/dashboard/AuditCards';
import { AutoRefresh } from '@/features/dashboard/AutoRefresh';
import { PageHeader } from '@/features/shell/PageHeader';
import { InferonApi } from '@/libs/InferonApi';

const WINDOWS = [7, 30, 90] as const;

/**
 * The public, read-only cost audit behind a share token. No session: the token
 * resolves to the tenant server-side, and the report is aggregates only.
 */
export default async function PublicAuditPage(props: {
  params: Promise<{ locale: string; token: string }>;
  searchParams: Promise<{ days?: string }>;
}) {
  const { locale, token } = await props.params;
  setRequestLocale(locale);

  const requested = Number((await props.searchParams).days);
  const days = (WINDOWS as readonly number[]).includes(requested) ? requested : 30;
  const t = await getTranslations({ locale, namespace: 'AuditPage' });
  const report = await InferonApi.fromEnv()
    .publicAudit(token, days)
    .catch(() => null);

  if (!report) {
    return (
      <main className="mx-auto max-w-xl p-10 text-center">
        <h1 className="text-2xl font-semibold">{t('invalid_title')}</h1>
        <p className="mt-2 text-sm text-muted-foreground">{t('invalid_body')}</p>
      </main>
    );
  }

  const data: AuditData = {
    requests: report.summary.requests,
    cost: report.summary.total_cost_usd,
    savings: {
      total: report.summary.total_savings_usd,
      router: report.summary.router_savings_usd,
      fallback: report.summary.fallback_savings_usd,
      cache: report.summary.cache_savings_usd,
      affinity: report.summary.cache_affinity_requests,
    },
    waste: report.waste,
    top: {
      model: report.top.model
        ? { name: report.top.model.name, cost: report.top.model.cost_usd }
        : undefined,
      provider: report.top.provider
        ? { name: report.top.provider.name, cost: report.top.provider.cost_usd }
        : undefined,
      key: report.top.key
        ? { name: report.top.key.name, cost: report.top.key.cost_usd }
        : undefined,
    },
    quality: {
      errorRate: report.quality.error_rate,
      truncatedRate: report.quality.truncated_rate,
      guardFallbackRate: report.quality.guard_fallback_rate,
      topFailure: Object.entries(report.quality.errors_by_class).sort((a, b) => b[1] - a[1])[0],
    },
  };

  return (
    <main className="
      mx-auto max-w-5xl p-6
      lg:p-10
    "
    >
      <div className="flex flex-wrap items-start justify-between gap-4">
        <PageHeader title={t('public_title')} description={t('public_body')} />
        <AutoRefresh label={t('live')} />
      </div>
      <AuditCards locale={locale} data={data} />
    </main>
  );
}
