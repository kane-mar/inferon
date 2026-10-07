'use client';

import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { centsToUsd, formatCompact, formatInt, moneyDeltaPct, spiked, trend } from '@/libs/dashboardMetrics';
import { formatUsd, InferonApi, InferonApiError } from '@/libs/InferonApi';

export type SpendBreakdownRow = {
  /** key_id / provider_id — `null` for a legacy/shared row (not editable). */
  id: string | null;
  label: string;
  meta?: string;
  costUsd: string;
  previousCostUsd?: string | null;
  requests: number;
  tokens: number;
  limitCents: number;
};

const INPUT = `
  w-24 rounded-md border bg-background px-2 py-1 font-mono text-xs outline-none
  focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50
`;

/**
 * ITEM-047: a spend breakdown with period delta, an inline spike badge (AC3)
 * and an editable monthly cap (AC2). Keys save through the key-limit endpoint;
 * providers through the provider update.
 */
export const SpendBreakdown = (props: {
  kind: 'key' | 'provider';
  rows: SpendBreakdownRow[];
  canEdit: boolean;
  emptyLabel: string;
}) => {
  const t = useTranslations('DashboardIndexPage');
  const router = useRouter();
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const saveLimit = async (id: string) => {
    const cents = Math.max(0, Math.round((Number(drafts[id]) || 0) * 100));
    setBusy(id);
    setError(null);
    try {
      if (props.kind === 'key') {
        await InferonApi.sameOrigin().setKeyLimit(id, cents);
      } else {
        await InferonApi.sameOrigin().updateProvider(id, { spend_limit_cents: cents });
      }
      router.refresh();
    } catch (cause) {
      setError(cause instanceof InferonApiError ? cause.message : t('limit_error'));
    } finally {
      setBusy(null);
    }
  };

  if (props.rows.length === 0) {
    return <p className="text-sm text-muted-foreground">{props.emptyLabel}</p>;
  }

  return (
    <div className="space-y-3">
      {error && <p className="text-sm text-destructive">{error}</p>}
      <table className="w-full text-left text-sm">
        <thead>
          <tr className="border-b text-muted-foreground">
            <th className="
              py-2 font-mono text-xs font-medium tracking-wide uppercase
            "
            >
              {props.kind === 'key' ? t('key') : t('provider')}
            </th>
            <th className="
              py-2 text-right font-mono text-xs font-medium tracking-wide
              uppercase
            "
            >
              {t('requests')}
            </th>
            <th className="
              py-2 text-right font-mono text-xs font-medium tracking-wide
              uppercase
            "
            >
              {t('tokens')}
            </th>
            <th className="
              py-2 text-right font-mono text-xs font-medium tracking-wide
              uppercase
            "
            >
              {t('cost')}
            </th>
            <th className="
              py-2 text-right font-mono text-xs font-medium tracking-wide
              uppercase
            "
            >
              {t('limit')}
            </th>
          </tr>
        </thead>
        <tbody>
          {props.rows.map((row) => {
            const delta = row.previousCostUsd == null
              ? null
              : moneyDeltaPct(row.costUsd, row.previousCostUsd);
            const isSpike = spiked(row.costUsd, row.previousCostUsd);
            const over = row.limitCents > 0 && Number(row.costUsd) * 100 >= row.limitCents;
            const editing = row.id != null && row.id in drafts;
            return (
              <tr
                key={row.id ?? row.label}
                className="
                  border-b
                  last:border-0
                "
              >
                <td className="py-2">
                  <span className="font-medium">{row.label}</span>
                  {row.meta && (
                    <span className="
                      ml-2 rounded-sm bg-secondary px-1.5 py-0.5 font-mono
                      text-xs
                    "
                    >
                      {row.meta}
                    </span>
                  )}
                  {isSpike && (
                    <span className="
                      ml-2 rounded-sm bg-destructive/10 px-1.5 py-0.5 font-mono
                      text-xs text-destructive
                    "
                    >
                      {t('spike')}
                    </span>
                  )}
                </td>
                <td className="py-2 text-right tabular-nums">{formatInt(row.requests)}</td>
                <td className="py-2 text-right tabular-nums">{formatCompact(row.tokens)}</td>
                <td className="py-2 text-right tabular-nums">
                  {formatUsd(row.costUsd)}
                  {delta != null && (
                    <span
                      className={`
                        ml-1 font-mono text-xs
                        ${
                    trend(delta) === 'up'
                      ? 'text-destructive'
                      : 'text-muted-foreground'
                    }
                      `}
                    >
                      {delta > 0 ? '+' : ''}
                      {delta.toFixed(0)}
                      %
                    </span>
                  )}
                </td>
                <td className="py-2 text-right">
                  {row.limitCents > 0 && (
                    <span
                      className={`
                        mr-2 font-mono text-xs tabular-nums
                        ${over ? 'text-destructive' : 'text-muted-foreground'}
                      `}
                    >
                      {formatUsd(centsToUsd(row.limitCents))}
                    </span>
                  )}
                  {props.canEdit && row.id != null && (
                    editing
                      ? (
                          <span className="inline-flex items-center gap-1">
                            <input
                              className={INPUT}
                              inputMode="decimal"
                              value={drafts[row.id] ?? ''}
                              placeholder={centsToUsd(row.limitCents)}
                              onChange={event => setDrafts(current => ({ ...current, [row.id as string]: event.target.value }))}
                            />
                            <Button
                              size="sm"
                              disabled={busy === row.id}
                              onClick={() => saveLimit(row.id as string)}
                            >
                              {t('save')}
                            </Button>
                            <Button
                              size="sm"
                              variant="ghost"
                              onClick={() => setDrafts((current) => {
                                const next = { ...current };
                                delete next[row.id as string];
                                return next;
                              })}
                            >
                              {t('cancel')}
                            </Button>
                          </span>
                        )
                      : (
                          <Button
                            size="sm"
                            variant="ghost"
                            onClick={() => setDrafts(current => ({ ...current, [row.id as string]: centsToUsd(row.limitCents) }))}
                          >
                            {row.limitCents > 0 ? t('edit_limit') : t('set_limit')}
                          </Button>
                        )
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
};
