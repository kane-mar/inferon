'use client';

import type { KeysResponse } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { errorMessage } from '@/libs/errors';
import { InferonApi } from '@/libs/InferonApi';

const date = (value: string | null) => (value ? value.slice(0, 10) : '—');

export const KeysPanel = (props: { data: KeysResponse }) => {
  const t = useTranslations('KeysPage');
  const router = useRouter();
  const [label, setLabel] = useState('');
  const [created, setCreated] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const mint = async (event: React.FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const minted = await InferonApi.sameOrigin().createKey(label);
      setCreated(minted.key);
      setLabel('');
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('error')));
    } finally {
      setBusy(false);
    }
  };

  const revoke = async (keyId: string) => {
    setBusy(true);
    setError(null);
    try {
      await InferonApi.sameOrigin().revokeKey(keyId);
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('error')));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-6">
      <form onSubmit={mint} className="flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1 text-sm">
          <span className="text-muted-foreground">{t('label_field')}</span>
          <input
            className="rounded-sm border bg-background px-3 py-2"
            value={label}
            onChange={event => setLabel(event.target.value)}
            placeholder={t('label_placeholder')}
            required
            maxLength={64}
          />
        </label>

        <button
          type="submit"
          disabled={busy}
          className="
            rounded-sm bg-primary px-3 py-2 text-sm font-medium
            text-primary-foreground
            disabled:opacity-50
          "
        >
          {t('create')}
        </button>
      </form>

      {created
        ? (
            <div className="
              rounded-sm border border-amber-400 bg-amber-50 p-3 text-sm
              text-amber-950
            "
            >
              <div className="font-medium">{t('key_created')}</div>
              <code className="mt-1 block break-all">{created}</code>
              <div className="mt-1 text-xs opacity-80">{t('key_once')}</div>
            </div>
          )
        : null}

      {error ? <p className="text-sm text-red-600">{error}</p> : null}

      {props.data.keys.length === 0
        ? (
            <p className="text-sm text-muted-foreground">{t('no_keys')}</p>
          )
        : (
            <div className="overflow-x-auto">
              <table className="w-full text-left text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground">
                    <th className="py-2 font-medium">{t('label_field')}</th>
                    <th className="py-2 font-medium">{t('created')}</th>
                    <th className="py-2 font-medium">{t('last_used')}</th>
                    <th className="py-2 font-medium">{t('status_label')}</th>
                    <th className="py-2" />
                  </tr>
                </thead>
                <tbody>
                  {props.data.keys.map(key => (
                    <tr
                      key={key.id}
                      className="
                        border-b
                        last:border-0
                      "
                    >
                      <td className="py-2">{key.label}</td>
                      <td className="py-2">{date(key.created_at)}</td>
                      <td className="py-2">{date(key.last_used_at)}</td>
                      <td className="py-2">
                        {key.revoked ? t('revoked') : t('active')}
                      </td>
                      <td className="py-2 text-right">
                        {key.revoked
                          ? null
                          : (
                              <button
                                type="button"
                                disabled={busy}
                                onClick={() => revoke(key.id)}
                                className="
                                  text-sm text-red-600
                                  hover:underline
                                  disabled:opacity-50
                                "
                              >
                                {t('revoke')}
                              </button>
                            )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
    </div>
  );
};
