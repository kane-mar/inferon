'use client';

import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { centsToUsd, formatCompact, formatInt, moneyDeltaPct, spiked, trend } from '@/libs/dashboardMetrics';
import { errorMessage } from '@/libs/errors';
import { formatUsd, InferonApi } from '@/libs/InferonApi';
import { cn } from '@/utils/Helpers';

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
  /** ITEM-061: provider health (absent for key rows). */
  errorRate?: number | null;
  p95Ms?: number | null;
};

const COMPACT_INPUT = `
  w-24 rounded-md border bg-background px-2 py-1 font-mono text-xs outline-none
  focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50
`;

const pctOrDash = (value: number | null | undefined): string =>
  (value == null ? '—' : `${(value * 100).toFixed(1)}%`);

const msOrDash = (value: number | null | undefined): string =>
  (value == null ? '—' : `${Math.round(value)}ms`);

const Th = (props: { children: React.ReactNode; align?: 'left' | 'right' }) => (
  <th
    className={cn(
      'py-2 font-mono text-xs font-medium tracking-wide uppercase',
      props.align === 'right' && 'text-right',
    )}
  >
    {props.children}
  </th>
);

/** Inline editor for one row's monthly cap; `onDone` closes it on save/cancel. */
const LimitEditor = (props: {
  kind: 'key' | 'provider';
  id: string;
  limitCents: number;
  onDone: () => void;
}) => {
  const t = useTranslations('DashboardIndexPage');
  const router = useRouter();
  const [draft, setDraft] = useState(() => centsToUsd(props.limitCents));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = async () => {
    const cents = Math.max(0, Math.round((Number(draft) || 0) * 100));
    setBusy(true);
    setError(null);
    try {
      if (props.kind === 'key') {
        await InferonApi.sameOrigin().setKeyLimit(props.id, cents);
      } else {
        await InferonApi.sameOrigin().updateProvider(props.id, { spend_limit_cents: cents });
      }
      props.onDone();
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('limit_error')));
    } finally {
      setBusy(false);
    }
  };

  return (
    <span className="inline-flex flex-wrap items-center justify-end gap-1">
      <input
        className={COMPACT_INPUT}
        inputMode="decimal"
        value={draft}
        onChange={event => setDraft(event.target.value)}
      />
      <Button size="sm" disabled={busy} onClick={save}>
        {t('save')}
      </Button>
      <Button size="sm" variant="ghost" disabled={busy} onClick={props.onDone}>
        {t('cancel')}
      </Button>
      {error && <span className="text-xs text-destructive">{error}</span>}
    </span>
  );
};

const LimitCell = (props: {
  kind: 'key' | 'provider';
  row: SpendBreakdownRow;
  canEdit: boolean;
}) => {
  const t = useTranslations('DashboardIndexPage');
  const [editing, setEditing] = useState(false);
  const { row } = props;
  const over = row.limitCents > 0 && Number(row.costUsd) * 100 >= row.limitCents;

  return (
    <td className="py-2 text-right">
      {row.limitCents > 0 && (
        <span
          className={cn(
            'mr-2 font-mono text-xs tabular-nums',
            over ? 'text-destructive' : 'text-muted-foreground',
          )}
        >
          {formatUsd(centsToUsd(row.limitCents))}
        </span>
      )}

      {props.canEdit && row.id != null && (editing
        ? (
            <LimitEditor
              kind={props.kind}
              id={row.id}
              limitCents={row.limitCents}
              onDone={() => setEditing(false)}
            />
          )
        : (
            <Button size="sm" variant="ghost" onClick={() => setEditing(true)}>
              {row.limitCents > 0 ? t('edit_limit') : t('set_limit')}
            </Button>
          ))}
    </td>
  );
};

const TrendDelta = (props: { delta: number }) => (
  <span
    className={cn(
      'ml-1 font-mono text-xs',
      trend(props.delta) === 'up' ? 'text-destructive' : 'text-muted-foreground',
    )}
  >
    {props.delta > 0 ? '+' : ''}
    {props.delta.toFixed(0)}
    %
  </span>
);

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

  if (props.rows.length === 0) {
    return <p className="text-sm text-muted-foreground">{props.emptyLabel}</p>;
  }

  return (
    <table className="w-full text-left text-sm">
      <thead>
        <tr className="border-b text-muted-foreground">
          <Th>{props.kind === 'key' ? t('key') : t('provider')}</Th>
          <Th align="right">{t('requests')}</Th>
          {props.kind === 'provider' && (
            <>
              <Th align="right">{t('error_rate')}</Th>
              <Th align="right">{t('p95_ttft')}</Th>
            </>
          )}
          <Th align="right">{t('tokens')}</Th>
          <Th align="right">{t('cost')}</Th>
          <Th align="right">{t('limit')}</Th>
        </tr>
      </thead>
      <tbody>
        {props.rows.map((row) => {
          const delta = row.previousCostUsd == null
            ? null
            : moneyDeltaPct(row.costUsd, row.previousCostUsd);
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
                    ml-2 rounded-sm bg-secondary px-1.5 py-0.5 font-mono text-xs
                  "
                  >
                    {row.meta}
                  </span>
                )}
                {spiked(row.costUsd, row.previousCostUsd) && (
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
              {props.kind === 'provider' && (
                <>
                  <td className="py-2 text-right tabular-nums">{pctOrDash(row.errorRate)}</td>
                  <td className="py-2 text-right tabular-nums">{msOrDash(row.p95Ms)}</td>
                </>
              )}
              <td className="py-2 text-right tabular-nums">{formatCompact(row.tokens)}</td>
              <td className="py-2 text-right tabular-nums">
                {formatUsd(row.costUsd)}
                {delta != null && <TrendDelta delta={delta} />}
              </td>
              <LimitCell kind={props.kind} row={row} canEdit={props.canEdit} />
            </tr>
          );
        })}
      </tbody>
    </table>
  );
};
