'use client';

import type { EdgeNodes } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { errorMessage } from '@/libs/errors';
import { InferonApi } from '@/libs/InferonApi';
import { FIELD_INPUT } from './formStyles';

/**
 * ITEM-065: enroll and manage edge nodes. The token is shown once; only its
 * hash is stored server-side.
 */
export const EdgeNodesPanel = (props: { data: EdgeNodes }) => {
  const t = useTranslations('EdgePage');
  const router = useRouter();
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [token, setToken] = useState<string | null>(null);

  const enroll = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await InferonApi.sameOrigin().createEdgeNode(name);
      setToken(result.token);
      setName('');
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('enroll_error')));
    } finally {
      setBusy(false);
    }
  };

  const revoke = async (id: string) => {
    try {
      await InferonApi.sameOrigin().revokeEdgeNode(id);
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('revoke_error')));
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-end gap-2">
        <label className="text-sm">
          <span className="text-muted-foreground">{t('name')}</span>
          <input
            className={FIELD_INPUT}
            value={name}
            placeholder={t('name_placeholder')}
            onChange={event => setName(event.target.value)}
          />
        </label>
        <Button disabled={busy} onClick={enroll}>
          {t('enroll')}
        </Button>
      </div>

      {token && (
        <div className="rounded-md border bg-secondary/40 p-3 text-sm">
          <p className="font-medium">{t('token_once')}</p>
          <code className="mt-1 block font-mono text-xs break-all">{token}</code>
        </div>
      )}
      {error && <p className="text-sm text-destructive">{error}</p>}

      {props.data.nodes.length === 0
        ? (
            <p className="text-sm text-muted-foreground">{t('no_nodes')}</p>
          )
        : (
            <table className="w-full text-left text-sm">
              <thead>
                <tr className="border-b text-muted-foreground">
                  <th className="py-2 font-medium">{t('node')}</th>
                  <th className="py-2 font-medium">{t('last_seen')}</th>
                  <th className="py-2" />
                </tr>
              </thead>
              <tbody>
                {props.data.nodes.map(node => (
                  <tr
                    key={node.id}
                    className="
                      border-b
                      last:border-0
                    "
                  >
                    <td className="py-2">
                      {node.name}
                      <span className="
                        ml-2 font-mono text-xs text-muted-foreground
                      "
                      >
                        {node.id.slice(0, 8)}
                      </span>
                    </td>
                    <td className="py-2 text-muted-foreground">
                      {node.last_seen_at ?? t('never')}
                    </td>
                    <td className="py-2 text-right">
                      <Button size="sm" variant="ghost" onClick={() => revoke(node.id)}>
                        {t('revoke')}
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
    </div>
  );
};
