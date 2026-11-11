// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

'use client';

import type { EvalRun, EvalVerdict, GoldSet } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { dateTime } from '@/libs/datetime';
import { errorMessage } from '@/libs/errors';
import { InferonApi } from '@/libs/InferonApi';
import { FIELD_INPUT } from './formStyles';
import { percent } from './reportFormat';

const when = (value: string) => dateTime(value);

/** The gate's verdict, coloured by what it means. */
const VERDICT_CLASS: Record<EvalVerdict['verdict'], string> = {
  improved: 'text-success',
  unchanged: 'text-muted-foreground',
  regressed: 'text-destructive',
  insufficient: 'text-muted-foreground',
};

/**
 * ITEM-098: pin a gold set from qualified examples, run a model against it, and
 * compare two runs. Eval traffic never enters the production aggregates, so a
 * run cannot move spend, routing or policy.
 */
export const EvalsPanel = (props: {
  goldSets: GoldSet[];
  runs: EvalRun[];
  canEdit: boolean;
}) => {
  const t = useTranslations('EvalsPage');
  const router = useRouter();
  const [cases, setCases] = useState('20');
  const [model, setModel] = useState('');
  const [baseline, setBaseline] = useState('');
  const [challenger, setChallenger] = useState('');
  const [verdict, setVerdict] = useState<EvalVerdict | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async (name: string, action: () => Promise<unknown>) => {
    setBusy(name);
    setError(null);
    try {
      await action();
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('error')));
    } finally {
      setBusy(null);
    }
  };

  const finished = props.runs.filter(run => run.status !== 'running');

  return (
    <div className="space-y-6">
      {error && <p className="text-sm text-destructive">{error}</p>}
      {!props.canEdit && (
        <p className="text-sm text-muted-foreground">{t('owner_only')}</p>
      )}

      {/* Gold sets */}
      <section className="space-y-2">
        <h3 className="text-sm font-medium">{t('gold_sets')}</h3>
        {props.goldSets.length === 0
          ? <p className="text-sm text-muted-foreground">{t('no_gold')}</p>
          : (
              <ul className="space-y-1 text-sm">
                {props.goldSets.map(set => (
                  <li
                    key={set.gold_set_id}
                    className="flex items-baseline justify-between gap-3"
                  >
                    <span className="font-mono text-xs">
                      v
                      {set.version}
                    </span>
                    <span className="text-muted-foreground">
                      {t('gold_row', { cases: set.cases, when: when(set.created_at) })}
                    </span>
                  </li>
                ))}
              </ul>
            )}
        {props.canEdit && (
          <form
            className="flex flex-wrap items-end gap-2 pt-1"
            onSubmit={(event) => {
              event.preventDefault();
              run('pin', () => InferonApi.sameOrigin().pinGoldSet(Math.max(1, Number(cases) || 20)));
            }}
          >
            <label className="text-sm">
              <span className="text-muted-foreground">{t('cases')}</span>
              <input
                className={FIELD_INPUT}
                inputMode="numeric"
                value={cases}
                onChange={event => setCases(event.target.value)}
              />
            </label>
            <Button type="submit" disabled={busy === 'pin'}>
              {busy === 'pin' ? t('pinning') : t('pin')}
            </Button>
          </form>
        )}
      </section>

      {/* One run */}
      <section className="space-y-2 border-t pt-4">
        <h3 className="text-sm font-medium">{t('run_a_model')}</h3>
        {props.canEdit && (
          <form
            className="flex flex-wrap items-end gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              if (model.trim() === '') {
                return;
              }
              run('run', () => InferonApi.sameOrigin().runEval({ model: model.trim() }));
            }}
          >
            <label className="min-w-64 text-sm">
              <span className="text-muted-foreground">{t('model')}</span>
              <input
                className={FIELD_INPUT}
                placeholder={t('model_placeholder')}
                value={model}
                onChange={event => setModel(event.target.value)}
              />
            </label>
            <Button type="submit" disabled={busy === 'run'}>
              {busy === 'run' ? t('starting') : t('start')}
            </Button>
          </form>
        )}
      </section>

      {/* Runs */}
      <section className="space-y-2 border-t pt-4">
        <h3 className="text-sm font-medium">{t('runs')}</h3>
        {props.runs.length === 0
          ? <p className="text-sm text-muted-foreground">{t('no_runs')}</p>
          : (
              <div className="overflow-x-auto">
                <table className="w-full text-left text-sm">
                  <thead>
                    <tr className="border-b text-muted-foreground">
                      <th className="py-2 font-medium">{t('model')}</th>
                      <th className="py-2 font-medium">{t('status')}</th>
                      <th className="py-2 font-medium">{t('cases')}</th>
                      <th className="py-2 font-medium">{t('pass_rate')}</th>
                      <th className="py-2 font-medium">{t('cost')}</th>
                      <th className="py-2 font-medium">{t('when')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {props.runs.map(run => (
                      <tr
                        key={run.run_id}
                        className="
                          border-b
                          last:border-0
                        "
                      >
                        <td className="py-2 font-mono text-xs">{run.model}</td>
                        <td className="py-2">{run.status}</td>
                        <td className="py-2 tabular-nums">
                          {run.scored}
                          /
                          {run.cases}
                        </td>
                        <td className="py-2 tabular-nums">{percent(run.pass_rate)}</td>
                        <td className="py-2 font-mono text-xs">{run.cost_usd}</td>
                        <td className="py-2 text-xs text-muted-foreground">
                          <span title={run.created_at}>{when(run.created_at)}</span>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
      </section>

      {/* The gate */}
      <section className="space-y-2 border-t pt-4">
        <h3 className="text-sm font-medium">{t('regression_gate')}</h3>
        {props.canEdit && finished.length >= 2 && (
          <form
            className="flex flex-wrap items-end gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              setVerdict(null);
              run('compare', async () => {
                const result = await InferonApi
                  .sameOrigin()
                  .compareEvals(baseline, challenger);
                setVerdict(result);
              });
            }}
          >
            <label className="min-w-56 text-sm">
              <span className="text-muted-foreground">{t('baseline')}</span>
              <select
                className={FIELD_INPUT}
                value={baseline}
                onChange={event => setBaseline(event.target.value)}
              >
                <option value="">—</option>
                {finished.map(run => (
                  <option key={run.run_id} value={run.run_id}>
                    {run.model}
                    {' · '}
                    {percent(run.pass_rate)}
                  </option>
                ))}
              </select>
            </label>
            <label className="min-w-56 text-sm">
              <span className="text-muted-foreground">{t('challenger')}</span>
              <select
                className={FIELD_INPUT}
                value={challenger}
                onChange={event => setChallenger(event.target.value)}
              >
                <option value="">—</option>
                {finished.map(run => (
                  <option key={run.run_id} value={run.run_id}>
                    {run.model}
                    {' · '}
                    {percent(run.pass_rate)}
                  </option>
                ))}
              </select>
            </label>
            <Button type="submit" disabled={busy === 'compare' || !baseline || !challenger}>
              {t('compare')}
            </Button>
          </form>
        )}
        {verdict && (
          <p className={`
            text-sm font-medium
            ${VERDICT_CLASS[verdict.verdict]}
          `}
          >
            {t('verdict', { verdict: verdict.verdict, detail: verdict.detail })}
          </p>
        )}
        {props.canEdit && finished.length < 2 && (
          <p className="text-xs text-muted-foreground">{t('need_two_runs')}</p>
        )}
      </section>
    </div>
  );
};
