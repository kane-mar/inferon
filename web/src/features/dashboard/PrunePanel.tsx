// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

'use client';

import type { PrunePolicy } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { errorMessage } from '@/libs/errors';
import { InferonApi } from '@/libs/InferonApi';
import { FIELD_INPUT } from './formStyles';

/**
 * ITEM-091: opt-in context pruning. Off unless the tenant publishes a policy;
 * every rewrite is traced and a holdout is sent unpruned so the effect is
 * measurable. A schema-contract request is never rewritten.
 */
export const PrunePanel = (props: { policy: PrunePolicy | null; canEdit: boolean }) => {
  const t = useTranslations('PruningPanel');
  const router = useRouter();
  const [enabled, setEnabled] = useState(props.policy?.enabled ?? false);
  const [duplicates, setDuplicates] = useState(props.policy?.drop_duplicate_turns ?? true);
  const [budget, setBudget] = useState(
    props.policy?.max_prompt_tokens == null ? '' : String(props.policy.max_prompt_tokens),
  );
  const [holdout, setHoldout] = useState(String(props.policy?.holdout_percent ?? 10));
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setSaved(false);
    const tokens = budget.trim() === '' ? null : Math.max(1, Math.round(Number(budget)));
    const policy: PrunePolicy = {
      enabled,
      drop_duplicate_turns: duplicates,
      max_prompt_tokens: tokens,
      holdout_percent: Math.min(50, Math.max(0, Math.round(Number(holdout) || 0))),
    };
    try {
      await InferonApi.sameOrigin().setPrune(policy);
      setSaved(true);
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('error')));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={save} className="space-y-3">
      {error && <p className="text-sm text-destructive">{error}</p>}

      <label className="flex items-center gap-2 text-sm font-medium">
        <input
          type="checkbox"
          className="size-4 accent-primary"
          checked={enabled}
          disabled={!props.canEdit}
          onChange={event => setEnabled(event.target.checked)}
        />
        {t('enable')}
      </label>
      <p className="text-xs text-muted-foreground">{t('help')}</p>

      <div className="space-y-2 pl-6">
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            className="size-4 accent-primary"
            checked={duplicates}
            disabled={!props.canEdit || !enabled}
            onChange={event => setDuplicates(event.target.checked)}
          />
          {t('duplicates')}
        </label>

        <label className="block text-sm">
          <span className="text-muted-foreground">{t('budget')}</span>
          <input
            className={FIELD_INPUT}
            inputMode="numeric"
            placeholder={t('budget_placeholder')}
            value={budget}
            disabled={!props.canEdit || !enabled}
            onChange={event => setBudget(event.target.value)}
          />
          <span className="mt-1 block text-xs text-muted-foreground">{t('budget_help')}</span>
        </label>

        <label className="block text-sm">
          <span className="text-muted-foreground">{t('holdout')}</span>
          <input
            className={FIELD_INPUT}
            inputMode="numeric"
            value={holdout}
            disabled={!props.canEdit || !enabled}
            onChange={event => setHoldout(event.target.value)}
          />
          <span className="mt-1 block text-xs text-muted-foreground">{t('holdout_help')}</span>
        </label>
      </div>

      {props.canEdit
        ? (
            <div className="flex items-center gap-3">
              <Button type="submit" disabled={busy}>
                {busy ? t('saving') : t('save')}
              </Button>
              {saved && (
                <span className="
                  text-sm text-success
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
