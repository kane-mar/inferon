'use client';

import type { TenantConfig } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { centsToUsd } from '@/libs/dashboardMetrics';
import { errorMessage } from '@/libs/errors';
import { formatUsd, InferonApi } from '@/libs/InferonApi';
import { FIELD_INPUT } from './formStyles';

export const ControlsPanel = (props: { data: TenantConfig; modelIds: string[] }) => {
  const t = useTranslations('ControlsPage');
  const router = useRouter();
  const { data } = props;
  const [budget, setBudget] = useState((data.budget_limit_cents / 100).toFixed(2));
  const [cheap, setCheap] = useState(data.cheap_fallback_model ?? '');
  const [guard, setGuard] = useState(data.guard_enabled);
  const [routerOn, setRouterOn] = useState(data.router_enabled);
  const [threshold, setThreshold] = useState(String(data.router_threshold));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const canEdit = data.role === 'owner';
  const modelIds = props.modelIds;

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setSaved(false);
    try {
      await InferonApi.sameOrigin().updateConfig({
        budget_limit_cents: Math.max(0, Math.round((Number(budget) || 0) * 100)),
        cheap_fallback_model: cheap.trim() || null,
        guard_enabled: guard,
        router_enabled: routerOn,
        router_threshold: Math.min(1, Math.max(0, Number(threshold) || 0)),
      });
      setSaved(true);
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('error')));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={save} className="space-y-5">
      {error && <p className="text-sm text-destructive">{error}</p>}

      <div className="space-y-1">
        <label className="text-sm">
          <span className="text-muted-foreground">{t('budget')}</span>
          <input
            className={FIELD_INPUT}
            inputMode="decimal"
            value={budget}
            disabled={!canEdit}
            onChange={event => setBudget(event.target.value)}
          />
        </label>
        <p className="text-xs text-muted-foreground">
          {data.budget_limit_cents > 0
            ? t('spent_of', {
                spent: formatUsd(centsToUsd(data.current_spend_cents)),
                limit: formatUsd(centsToUsd(data.budget_limit_cents)),
              })
            : t('spent_unlimited', { spent: formatUsd(centsToUsd(data.current_spend_cents)) })}
        </p>
      </div>

      <label className="block text-sm">
        <span className="text-muted-foreground">{t('cheap_fallback')}</span>
        <select
          className={FIELD_INPUT}
          value={cheap}
          disabled={!canEdit}
          onChange={event => setCheap(event.target.value)}
        >
          <option value="">{t('none')}</option>
          {modelIds.map(id => <option key={id} value={id}>{id}</option>)}
        </select>
        <span className="mt-1 block text-xs text-muted-foreground">{t('cheap_help')}</span>
      </label>

      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          className="size-4 accent-primary"
          checked={guard}
          disabled={!canEdit}
          onChange={event => setGuard(event.target.checked)}
        />
        {t('guard')}
      </label>

      <div className="space-y-1">
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            className="size-4 accent-primary"
            checked={routerOn}
            disabled={!canEdit}
            onChange={event => setRouterOn(event.target.checked)}
          />
          {t('router')}
        </label>
        <label className="block text-sm">
          <span className="text-muted-foreground">{t('threshold')}</span>
          <input
            className={FIELD_INPUT}
            inputMode="decimal"
            value={threshold}
            disabled={!canEdit || !routerOn}
            onChange={event => setThreshold(event.target.value)}
          />
          <span className="mt-1 block text-xs text-muted-foreground">{t('threshold_help')}</span>
        </label>
      </div>

      {canEdit
        ? (
            <div className="flex items-center gap-3">
              <Button type="submit" disabled={busy}>
                {busy ? t('saving') : t('save')}
              </Button>
              {saved && (
                <span className="
                  text-sm text-emerald-600
                  dark:text-emerald-400
                "
                >
                  {t('saved')}
                </span>
              )}
            </div>
          )
        : (
            <p className="text-sm text-muted-foreground">{t('owner_only')}</p>
          )}
    </form>
  );
};
