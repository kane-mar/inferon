// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { getTranslations } from 'next-intl/server';
import { errorClassKeys } from '@/libs/labels';
import { formatInt } from '@/libs/dashboardMetrics';
import { Finding } from './Finding';
import { money } from './money';
import { pct } from './reportFormat';

/** A waste flag as both the dashboard and the public report expose it. */
export type WasteEntry = {
  requests: number;
  spend_usd: string;
  savings_est_usd: string;
};

/** Everything the six cards need, independent of where it was loaded from. */
export type AuditData = {
  requests: number;
  cost: string;
  savings: {
    total: string;
    router: string;
    fallback: string;
    cache: string;
    affinity: number;
  };
  waste: Record<string, WasteEntry>;
  top: {
    model?: { name: string; cost: string };
    provider?: { name: string; cost: string };
    key?: { name: string; cost: string };
  };
  quality: {
    errorRate: number | null;
    truncatedRate: number | null;
    guardFallbackRate: number | null;
    topFailure?: [string, number];
  };
};

/**
 * The six named findings, shared by the tenant's live page and the read-only
 * public report, so both always show the same thing.
 */
export const AuditCards = async (props: { locale: string; data: AuditData }) => {
  const { locale, data } = props;
  const t = await getTranslations({ locale, namespace: 'AuditPage' });
  // ITEM-125: name the failure in words; the code stays beside the count.
  const causes = await getTranslations({ locale, namespace: 'ErrorClass' });
  const failure = (() => {
    const top = data.quality.topFailure;
    if (!top) {
      return t('none');
    }
    const key = errorClassKeys(top[0]);
    return key ? `${causes(key)} · ${formatInt(top[1])}` : `${top[0]} · ${formatInt(top[1])}`;
  })();
  const over = data.waste.over_provisioned;
  const misuse = data.waste.model_misuse;
  const cached = data.waste.cache_miss;

  const named = (row?: { name: string; cost: string }): string =>
    row ? `${row.name} · ${money(row.cost)}` : t('none');

  return (
    <div className="
      grid grid-cols-1 gap-4
      lg:grid-cols-2
    "
    >
      <Finding
        title={t('f1_title')}
        highlight={money(over?.savings_est_usd ?? '0')}
        metrics={[
          { label: t('requests'), value: formatInt(over?.requests ?? 0) },
          { label: t('spend'), value: money(over?.spend_usd ?? '0') },
          { label: t('recoverable'), value: money(over?.savings_est_usd ?? '0') },
        ]}
      />
      <Finding
        title={t('f2_title')}
        highlight={money(misuse?.savings_est_usd ?? '0')}
        metrics={[
          { label: t('requests'), value: formatInt(misuse?.requests ?? 0) },
          { label: t('spend'), value: money(misuse?.spend_usd ?? '0') },
          { label: t('recoverable'), value: money(misuse?.savings_est_usd ?? '0') },
        ]}
      />
      <Finding
        title={t('f3_title')}
        highlight={money(cached?.spend_usd ?? '0')}
        metrics={[
          { label: t('requests'), value: formatInt(cached?.requests ?? 0) },
          { label: t('spend'), value: money(cached?.spend_usd ?? '0') },
        ]}
      />
      <Finding
        title={t('f4_title')}
        highlight={money(data.savings.total)}
        metrics={[
          { label: t('router'), value: money(data.savings.router) },
          { label: t('fallback'), value: money(data.savings.fallback) },
          { label: t('cache'), value: money(data.savings.cache) },
          { label: t('affinity_requests'), value: formatInt(data.savings.affinity) },
        ]}
      />
      <Finding
        title={t('f5_title')}
        highlight={money(data.cost)}
        metrics={[
          { label: t('top_model'), value: named(data.top.model) },
          { label: t('top_provider'), value: named(data.top.provider) },
          { label: t('top_key'), value: named(data.top.key) },
        ]}
      />
      <Finding
        title={t('f6_title')}
        highlight={pct(data.quality.errorRate)}
        metrics={[
          { label: t('truncated_rate'), value: pct(data.quality.truncatedRate) },
          { label: t('guard_fallback_rate'), value: pct(data.quality.guardFallbackRate) },
          {
            label: t('top_error_class'),
            value: failure,
          },
        ]}
      />
    </div>
  );
};
