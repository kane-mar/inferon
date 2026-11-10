// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

'use client';

import type { Auth, OperatorPolicy, PromptEvidence } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useEffect, useState } from 'react';
import { InferonApi } from '@/libs/InferonApi';
import { Card } from './Card';
import { money } from './money';

/**
 * ITEM-139: steering the adaptive loop.
 *
 * The loop adjusts itself from telemetry; this is where a human sees the version
 * in force, why it exists, and changes it. A change publishes a new version with
 * `evidence.source = "operator"`, so the same rollback that undoes a bad automatic
 * step undoes a bad manual one. The score shows the decision each request makes:
 * threshold = how readily the router may downgrade.
 */
export const Adaptation = (props: { tenants: { tenant_id: string; requests: number }[]; auth: Auth }) => {
  const t = useTranslations('OperatorPage');
  const router = useRouter();
  const [tenantId, setTenantId] = useState(props.tenants[0]?.tenant_id ?? '');
  const [policy, setPolicy] = useState<OperatorPolicy | null>(null);
  const [threshold, setThreshold] = useState('');
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!tenantId) {
      return;
    }
    InferonApi.fromEnv()
      .operatorPolicy(tenantId, props.auth)
      .then((result) => {
        setPolicy(result.policy);
        setThreshold(result.policy?.router_threshold?.toFixed(2) ?? '');
      })
      .catch(() => setPolicy(null));
  }, [tenantId, props.auth]);

  const act = async (run: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await run();
      router.refresh();
    } finally {
      setBusy(false);
    }
  };

  const api = InferonApi.fromEnv();
  const weights = Object.entries(policy?.provider_weights ?? {});
  // ITEM-141: the loop's prompt observations arrive as policy evidence.
  const prompt = (policy?.evidence as { prompt?: PromptEvidence } | undefined)?.prompt;
  const objective = policy?.objective ?? 'balanced';

  return (
    <Card title={t('adaptation')} icon="activity">
      <div className="flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1 text-xs text-muted-foreground">
          {t('workspace')}
          <select
            className="rounded-sm border bg-card px-2 py-1 text-sm"
            value={tenantId}
            onChange={event => setTenantId(event.target.value)}
          >
            {props.tenants.map(tenant => (
              <option key={tenant.tenant_id} value={tenant.tenant_id}>
                {tenant.tenant_id.slice(0, 8)}
                {' · '}
                {tenant.requests}
              </option>
            ))}
          </select>
        </label>

        <label className="flex flex-col gap-1 text-xs text-muted-foreground">
          {t('threshold')}
          <input
            className="
              w-24 rounded-sm border bg-card px-2 py-1 text-sm tabular-nums
            "
            inputMode="decimal"
            value={threshold}
            onChange={event => setThreshold(event.target.value)}
          />
        </label>

        <label className="flex flex-col gap-1 text-xs text-muted-foreground">
          {t('objective')}
          <select
            className="rounded-sm border bg-card px-2 py-1 text-sm"
            value={objective}
            onChange={event => act(() => api.operatorPolicySet(
              tenantId,
              { objective: event.target.value as 'cost' | 'balanced' | 'quality' },
              props.auth,
            ))}
          >
            <option value="cost">{t('objective_cost')}</option>
            <option value="balanced">{t('objective_balanced')}</option>
            <option value="quality">{t('objective_quality')}</option>
          </select>
        </label>

        <button
          type="button"
          disabled={busy || !tenantId || threshold === ''}
          className="
            rounded-sm border px-3 py-1.5 text-sm font-medium
            disabled:opacity-40
          "
          onClick={() => act(() => api.operatorPolicySet(tenantId, { router_threshold: Number(threshold) }, props.auth))}
        >
          {t('apply')}
        </button>

        <button
          type="button"
          disabled={busy || !tenantId}
          className="
            rounded-sm border px-3 py-1.5 text-sm font-medium
            disabled:opacity-40
          "
          onClick={() => act(() => api.operatorPolicyFreeze(tenantId, props.auth))}
        >
          {t('freeze')}
        </button>

        {policy && policy.version > 1 && (
          <button
            type="button"
            disabled={busy || !tenantId}
            className="
              rounded-sm border px-3 py-1.5 text-sm font-medium
              disabled:opacity-40
            "
            onClick={() => act(() => api.operatorPolicyRollback(tenantId, policy.version - 1, props.auth))}
          >
            {t('rollback')}
          </button>
        )}
      </div>

      <dl className="
        mt-4 grid grid-cols-2 gap-x-6 gap-y-2 text-sm
        sm:grid-cols-3
      "
      >
        <div>
          <dt className="text-xs text-muted-foreground">{t('version')}</dt>
          <dd className="tabular-nums">{policy ? policy.version : '—'}</dd>
        </div>
        <div>
          <dt className="text-xs text-muted-foreground">{t('state')}</dt>
          <dd>{policy ? (policy.enabled ? t('adapting') : t('frozen')) : '—'}</dd>
        </div>
        <div>
          <dt className="text-xs text-muted-foreground">{t('weights')}</dt>
          <dd className="tabular-nums">
            {weights.length === 0 ? t('no_opinion') : weights.map(([id, w]) => `${id.slice(0, 8)} ${w.toFixed(2)}`).join(' · ')}
          </dd>
        </div>
      </dl>

      {prompt && (
        <div className="mt-4 border-t pt-3 text-sm">
          <div className="flex flex-wrap items-baseline justify-between gap-2">
            <div className="flex items-baseline gap-2">
              <span className="text-muted-foreground">{t('prompt_waste')}</span>
              <span className="tabular-nums">{money(prompt.waste_usd)}</span>
              <code className="text-xs text-muted-foreground">{prompt.top_kind ?? '—'}</code>
            </div>
            {prompt.propose_pruning && !prompt.pruning_enabled && (
              <button
                type="button"
                disabled={busy}
                className="
                  rounded-sm border px-3 py-1.5 text-xs font-medium
                  disabled:opacity-40
                "
                onClick={() => act(() => api.pruneSet({
                  enabled: true,
                  drop_duplicate_turns: true,
                  max_prompt_tokens: null,
                  holdout_percent: 10,
                }, props.auth))}
              >
                {t('enable_pruning')}
              </button>
            )}
            {prompt.pruning_enabled && (
              <span className="text-xs text-muted-foreground">{t('pruning_on')}</span>
            )}
          </div>
        </div>
      )}

      {policy?.evidence && Object.keys(policy.evidence).length > 0 && (
        <code className="
          mt-3 block overflow-x-auto rounded-sm bg-secondary p-2 text-xs
        "
        >
          {JSON.stringify(policy.evidence)}
        </code>
      )}
    </Card>
  );
};

/**
 * ITEM-139: the adaptive loop is steered from here.
 *
 * Client-side because it is a form; the data it reads is operator-scoped, so the
 * page passes the session's auth through. `evidence` is printed raw on purpose —
 * it is the loop's own reasoning (version, sample count, what moved) and the
 * operator reading it is the one person who wants the JSON rather than a summary.
 */
