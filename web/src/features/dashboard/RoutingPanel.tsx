'use client';

import type { CatalogModel, RoutingResponse } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { InferonApi, InferonApiError } from '@/libs/InferonApi';

const errorMessage = (error: unknown, fallback: string) =>
  error instanceof InferonApiError ? error.message : fallback;

const SELECT = `
  rounded-md border bg-background px-3 py-2 text-sm outline-none
  focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50
`;

export const RoutingPanel = (props: { data: RoutingResponse; models: CatalogModel[] }) => {
  const t = useTranslations('RoutingPage');
  const router = useRouter();
  const [chain, setChain] = useState<string[]>(props.data.fallbacks);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const canEdit = props.data.role === 'owner';

  const candidates = props.models
    .map(model => model.id)
    .filter(id => !chain.includes(id));

  const save = async (next: string[]) => {
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
          <span className="text-xs text-muted-foreground">{t('primary_help')}</span>
        </li>

        {chain.map((modelId, index) => (
          <li
            key={modelId}
            className="
              flex items-center gap-3 rounded-lg border bg-card px-4 py-3
              text-sm
            "
          >
            <span className="font-mono text-xs text-muted-foreground">{index + 2}</span>
            <span className="flex-1 font-mono text-xs">{modelId}</span>
            {canEdit && (
              <Button
                variant="ghost"
                size="sm"
                disabled={busy}
                onClick={() => save(chain.filter(id => id !== modelId))}
              >
                {t('remove')}
              </Button>
            )}
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
              save([...chain, event.target.value]);
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
        <p className="
          text-sm text-emerald-600
          dark:text-emerald-400
        "
        >
          {t('saved')}
        </p>
      )}
      {!canEdit && <p className="text-sm text-muted-foreground">{t('owner_only')}</p>}
    </div>
  );
};
