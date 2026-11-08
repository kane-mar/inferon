// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

'use client';

import type { ActiveTierPair, TierPair } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { errorMessage } from '@/libs/errors';
import { InferonApi } from '@/libs/InferonApi';
import { FIELD_INPUT } from './formStyles';

/**
 * The tenant's own downshift pairs (ITEM-081): simple traffic on a frontier
 * model is served by the cheap model they name — including their own local one.
 * These win over the built-in table, and a pair whose models cannot both be
 * routed to right now is shown as inactive rather than quietly dropped.
 */
export const TierPairsPanel = (props: {
  pairs: ActiveTierPair[];
  modelIds: string[];
  canEdit: boolean;
}) => {
  const t = useTranslations('RoutingPage');
  const router = useRouter();
  const [frontier, setFrontier] = useState('');
  const [cheap, setCheap] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const pairs = props.pairs;
  const paired = new Set(pairs.map(pair => pair.frontier));
  const frontiers = props.modelIds.filter(id => !paired.has(id));
  const cheaps = props.modelIds.filter(id => id !== frontier);

  const save = async (next: TierPair[]) => {
    setBusy(true);
    setError(null);
    try {
      await InferonApi.sameOrigin().setTierPairs(next);
      setFrontier('');
      setCheap('');
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('tier_pairs_error')));
    } finally {
      setBusy(false);
    }
  };

  const add = () => {
    if (frontier && cheap) {
      save([...pairs.map(({ frontier, cheap }) => ({ frontier, cheap })), { frontier, cheap }]);
    }
  };

  return (
    <div className="space-y-4">
      {error && <p className="text-sm text-destructive">{error}</p>}
      <p className="text-sm text-muted-foreground">{t('tier_pairs_help')}</p>

      <ul className="space-y-2">
        {pairs.map(pair => (
          <li
            key={`${pair.frontier}→${pair.cheap}`}
            className="
              flex items-center gap-3 rounded-lg border bg-card px-4 py-3
              text-sm
            "
          >
            <span className="font-mono text-xs">{pair.frontier}</span>
            <span className="text-muted-foreground">→</span>
            <span className="flex-1 font-mono text-xs">{pair.cheap}</span>
            {!pair.active && (
              <span className="
                text-xs text-warning
              "
              >
                {t('tier_pairs_inactive')}
              </span>
            )}
            {props.canEdit && (
              <Button
                variant="ghost"
                size="sm"
                disabled={busy}
                onClick={() => save(
                  pairs
                    .filter(other => other.frontier !== pair.frontier)
                    .map(({ frontier, cheap }) => ({ frontier, cheap })),
                )}
              >
                {t('remove')}
              </Button>
            )}
          </li>
        ))}
        {pairs.length === 0 && (
          <li className="text-sm text-muted-foreground">{t('tier_pairs_empty')}</li>
        )}
      </ul>

      {props.canEdit && frontiers.length > 0 && (
        <div className="flex flex-wrap items-end gap-2">
          <label className="flex-1 text-sm">
            <span className="text-muted-foreground">{t('tier_pairs_frontier')}</span>
            <select
              className={FIELD_INPUT}
              value={frontier}
              disabled={busy}
              onChange={(event) => {
                setFrontier(event.target.value);
                setCheap('');
              }}
            >
              <option value="">{t('tier_pairs_select')}</option>
              {frontiers.map(id => (
                <option key={id} value={id}>{id}</option>
              ))}
            </select>
          </label>
          <label className="flex-1 text-sm">
            <span className="text-muted-foreground">{t('tier_pairs_cheap')}</span>
            <select
              className={FIELD_INPUT}
              value={cheap}
              disabled={busy || !frontier}
              onChange={event => setCheap(event.target.value)}
            >
              <option value="">{t('tier_pairs_select')}</option>
              {cheaps.map(id => (
                <option key={id} value={id}>{id}</option>
              ))}
            </select>
          </label>
          <Button type="button" disabled={busy || !frontier || !cheap} onClick={add}>
            {t('tier_pairs_add')}
          </Button>
        </div>
      )}

      {!props.canEdit && <p className="text-sm text-muted-foreground">{t('owner_only')}</p>}
    </div>
  );
};
