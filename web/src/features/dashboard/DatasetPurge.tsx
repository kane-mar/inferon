'use client';

import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { errorMessage } from '@/libs/errors';
import { InferonApi } from '@/libs/InferonApi';

/**
 * ITEM-088: delete every stored payload now. Costs and aggregates survive; this
 * is the "forget my data" action, separate from the retention window. Two-step
 * (arm, then confirm) instead of a browser confirm dialog.
 */
export const DatasetPurge = () => {
  const t = useTranslations('DatasetsPage');
  const router = useRouter();
  const [armed, setArmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [purged, setPurged] = useState<number | null>(null);

  const purge = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await InferonApi.sameOrigin().purgePayloads();
      setPurged(result.purged);
      setArmed(false);
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('purge_error')));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex items-center gap-2">
      {error && <p className="text-sm text-destructive">{error}</p>}
      {purged != null && !armed && (
        <p className="text-xs text-muted-foreground">{t('purged', { count: purged })}</p>
      )}
      {armed
        ? (
            <>
              <span className="text-xs text-destructive">{t('purge_confirm')}</span>
              <Button variant="outline" size="sm" disabled={busy} onClick={purge}>
                {busy ? t('purging') : t('purge_yes')}
              </Button>
              <Button variant="ghost" size="sm" disabled={busy} onClick={() => setArmed(false)}>
                {t('cancel')}
              </Button>
            </>
          )
        : (
            <Button variant="outline" size="sm" onClick={() => setArmed(true)}>
              {t('purge')}
            </Button>
          )}
    </div>
  );
};
