// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

'use client';

import type { FallbackTrigger, RoutingResponse, RoutingStep } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { errorMessage } from '@/libs/errors';
import { InferonApi } from '@/libs/InferonApi';
import { cn } from '@/utils/Helpers';

const SELECT = `
  rounded-md border bg-background px-3 py-2 text-sm outline-none
  focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50
`;

const RETRYABLE: FallbackTrigger[] = ['transport', 'rate_limited', 'server_error'];
const TRIGGERS: FallbackTrigger[] = [...RETRYABLE, 'guard'];

export const RoutingPanel = (props: { data: RoutingResponse; modelIds: string[] }) => {
  const t = useTranslations('RoutingPage');
  const router = useRouter();
  const [chain, setChain] = useState<RoutingStep[]>(props.data.fallbacks);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const canEdit = props.data.role === 'owner';

  const candidates = props.modelIds.filter(id => !chain.some(step => step.model === id));

  const save = async (next: RoutingStep[]) => {
    setBusy(true);
    setError(null);
    setSaved(false);
    try {
      await InferonApi.sameOrigin().setRouting(next);
      setChain(next);
      setSaved(true);
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('error')));
    } finally {
      setBusy(false);
    }
  };

  /** A step must keep at least one trigger — an empty step would never run. */
  const editTriggers = (step: RoutingStep, trigger: FallbackTrigger) => {
    const active = step.triggers.includes(trigger);
    const triggers = active
      ? step.triggers.filter(current => current !== trigger)
      : [...step.triggers, trigger];
    if (triggers.length === 0) {
      return;
    }
    save(chain.map(current => current.model === step.model ? { ...current, triggers } : current));
  };

  return (
    <div className="space-y-4">
      {error && <p className="text-sm text-destructive">{error}</p>}
      <p className="text-sm text-muted-foreground">{t('help')}</p>

      <ol className="space-y-2">
        <li className="
          flex items-center gap-3 rounded-lg border bg-card px-4 py-3 text-sm
        "
        >
          <span className="font-mono text-xs text-muted-foreground">1</span>
          <span className="font-medium">{t('primary')}</span>
          <span className="text-xs text-muted-foreground">{t('primary_hint')}</span>
        </li>

        {chain.map((step, index) => (
          <li
            key={step.model}
            className="rounded-lg border bg-card px-4 py-3 text-sm"
          >
            <div className="flex items-center gap-3">
              <span className="font-mono text-xs text-muted-foreground">{index + 2}</span>
              <span className="flex-1 font-mono text-xs">{step.model}</span>
              {canEdit && (
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={busy}
                  onClick={() => save(chain.filter(current => current.model !== step.model))}
                >
                  {t('remove')}
                </Button>
              )}
            </div>

            <div className="mt-2 flex flex-wrap items-center gap-1.5 pl-6">
              <span className="text-xs text-muted-foreground">{t('triggers')}</span>
              {TRIGGERS.map((trigger) => {
                const active = step.triggers.includes(trigger);
                return (
                  <button
                    key={trigger}
                    type="button"
                    aria-pressed={active}
                    disabled={!canEdit || busy || (active && step.triggers.length === 1)}
                    onClick={() => editTriggers(step, trigger)}
                    className={cn(
                      'rounded-full border px-2 py-0.5 text-xs',
                      active
                        ? `
                          border-primary bg-primary/10 font-medium text-primary
                        `
                        : `
                          text-muted-foreground
                          hover:text-foreground
                        `,
                      'disabled:opacity-50',
                    )}
                  >
                    {t(`trigger_${trigger}`)}
                  </button>
                );
              })}
            </div>
          </li>
        ))}

        {chain.length === 0 && (
          <li className="text-sm text-muted-foreground">{t('empty')}</li>
        )}
      </ol>

      {canEdit && candidates.length > 0 && (
        <select
          className={SELECT}
          value=""
          disabled={busy}
          onChange={(event) => {
            if (event.target.value) {
              save([...chain, { model: event.target.value, triggers: RETRYABLE }]);
            }
          }}
        >
          <option value="">{t('add_placeholder')}</option>
          {candidates.map(id => (
            <option key={id} value={id}>{id}</option>
          ))}
        </select>
      )}

      {saved && (
        <p className="text-sm text-success">
          {t('saved')}
        </p>
      )}
      {!canEdit && <p className="text-sm text-muted-foreground">{t('owner_only')}</p>}
    </div>
  );
};
