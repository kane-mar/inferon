'use client';

import type { AuditShares } from '@/libs/InferonApi';
import { useLocale, useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { errorMessage } from '@/libs/errors';
import { InferonApi } from '@/libs/InferonApi';

/**
 * Mint and revoke read-only audit links. The token is shown once (only its hash
 * is stored), so the panel hands over the full URL immediately after creation.
 */
export const ShareAuditPanel = (props: { data: AuditShares }) => {
  const t = useTranslations('AuditPage');
  const locale = useLocale();
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [url, setUrl] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const { token } = await InferonApi.sameOrigin().createAuditShare();
      setUrl(`${window.location.origin}/${locale}/audit/${token}`);
      setCopied(false);
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('share_error')));
    } finally {
      setBusy(false);
    }
  };

  const revoke = async (id: string) => {
    try {
      await InferonApi.sameOrigin().revokeAuditShare(id);
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('share_revoke_error')));
    }
  };

  const copy = async () => {
    if (!url) {
      return;
    }
    await navigator.clipboard?.writeText(url);
    setCopied(true);
  };

  return (
    <div className="space-y-3 text-sm">
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" disabled={busy} onClick={create}>
          {t('share_create')}
        </Button>
        {url && (
          <Button size="sm" variant="outline" onClick={copy}>
            {copied ? t('share_copied') : t('share_copy')}
          </Button>
        )}
      </div>

      {url && (
        <div className="rounded-md border bg-secondary/40 p-3">
          <p className="font-medium">{t('share_once')}</p>
          <code className="mt-1 block font-mono text-xs break-all">{url}</code>
        </div>
      )}
      {error && <p className="text-destructive">{error}</p>}

      {props.data.shares.length === 0
        ? (
            <p className="text-muted-foreground">{t('share_none')}</p>
          )
        : (
            <ul className="space-y-1">
              {props.data.shares.map(share => (
                <li
                  key={share.id}
                  className="
                    flex items-center justify-between gap-3 border-b py-1
                    last:border-0
                  "
                >
                  <span className="truncate">
                    {share.label || t('share_link')}
                    {share.revoked_at && (
                      <span className="ml-2 text-xs text-muted-foreground">
                        {t('share_revoked')}
                      </span>
                    )}
                  </span>
                  {!share.revoked_at && (
                    <Button size="sm" variant="ghost" onClick={() => revoke(share.id)}>
                      {t('share_revoke')}
                    </Button>
                  )}
                </li>
              ))}
            </ul>
          )}
    </div>
  );
};
