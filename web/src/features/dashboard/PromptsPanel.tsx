'use client';

import type { PromptDiff, PromptEntry } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { errorMessage } from '@/libs/errors';
import { InferonApi } from '@/libs/InferonApi';
import { FIELD_INPUT, MONO_FIELD_INPUT } from './formStyles';
import { money } from './money';

const pct = (value: number | null) => (value == null ? '—' : `${(value * 100).toFixed(0)}%`);
const short = (hash: string) => hash.slice(0, 10);

/** The three diff kinds, and how each reads. */
const DIFF_CLASS: Record<PromptDiff['changes'][number]['kind'], string> = {
  same: 'text-muted-foreground',
  added: 'text-emerald-700 dark:text-emerald-300',
  removed: 'text-destructive',
};

/**
 * A stable key per diff line: the line's own content plus its occurrence count,
 * so two identical lines stay distinct without keying on the array index.
 */
const withIds = (changes: PromptDiff['changes']) => {
  const seen = new Map<string, number>();
  return changes.map((change) => {
    const base = `${change.kind}:${change.text}`;
    const occurrence = (seen.get(base) ?? 0) + 1;
    seen.set(base, occurrence);
    return { ...change, id: `${base}#${occurrence}` };
  });
};

const DIFF_SIGN: Record<PromptDiff['changes'][number]['kind'], string> = {
  same: ' ',
  added: '+',
  removed: '−',
};

/**
 * ITEM-097: name a content hash, see its versions and outcomes, and diff two
 * versions. Read-only in v1 — a label never edits a prompt and never deletes a
 * trace.
 */
export const PromptsPanel = (props: {
  prompts: PromptEntry[];
  canEdit: boolean;
  /** False when payload capture is off: the counts exist, the content does not. */
  contentAvailable: boolean;
}) => {
  const t = useTranslations('PromptsPage');
  const router = useRouter();
  const [name, setName] = useState('');
  const [hash, setHash] = useState('');
  const [diffName, setDiffName] = useState('');
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [diff, setDiff] = useState<PromptDiff | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async (label: string, action: () => Promise<unknown>) => {
    setBusy(label);
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

  const versions = props.prompts.find(entry => entry.name === diffName)?.versions ?? [];

  return (
    <div className="space-y-6">
      {error && <p className="text-sm text-destructive">{error}</p>}

      {props.canEdit && (
        <form
          className="flex flex-wrap items-end gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            if (name.trim() === '' || hash.trim() === '') {
              return;
            }
            run('label', async () => {
              await InferonApi.sameOrigin().labelPrompt(name.trim(), hash.trim());
              setName('');
              setHash('');
            });
          }}
        >
          <label className="min-w-48 text-sm">
            <span className="text-muted-foreground">{t('name')}</span>
            <input
              className={FIELD_INPUT}
              placeholder={t('name_placeholder')}
              value={name}
              onChange={event => setName(event.target.value)}
            />
          </label>
          <label className="min-w-64 text-sm">
            <span className="text-muted-foreground">{t('prompt_hash')}</span>
            <input
              className={MONO_FIELD_INPUT}
              placeholder={t('hash_placeholder')}
              value={hash}
              onChange={event => setHash(event.target.value)}
            />
          </label>
          <Button type="submit" disabled={busy === 'label'}>
            {t('label')}
          </Button>
        </form>
      )}

      {props.prompts.length === 0
        ? <p className="text-sm text-muted-foreground">{t('none')}</p>
        : props.prompts.map(entry => (
            <section
              key={entry.name}
              className="
                space-y-2 border-t pt-4
                first:border-0
              "
            >
              <h3 className="font-mono text-sm font-medium">{entry.name}</h3>
              <div className="overflow-x-auto">
                <table className="w-full text-left text-sm">
                  <thead>
                    <tr className="border-b text-muted-foreground">
                      <th className="py-2 font-medium">{t('version')}</th>
                      <th className="py-2 font-medium">{t('requests')}</th>
                      <th className="py-2 font-medium">{t('ok_rate')}</th>
                      <th className="py-2 font-medium">{t('thumbs_down')}</th>
                      <th className="py-2 font-medium">{t('spend')}</th>
                      <th className="py-2 font-medium">{t('first_seen')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {entry.versions.map(version => (
                      <tr
                        key={version.prompt_hash}
                        className="
                          border-b
                          last:border-0
                        "
                      >
                        <td className="py-2 font-mono text-xs">
                          <span title={version.prompt_hash}>{short(version.prompt_hash)}</span>
                        </td>
                        <td className="py-2 tabular-nums">{version.requests}</td>
                        <td className="py-2 tabular-nums">{pct(version.ok_rate)}</td>
                        <td className="py-2 tabular-nums">{version.thumbs_down}</td>
                        <td className="py-2 font-mono text-xs">{money(version.spend_usd)}</td>
                        <td className="py-2 text-xs text-muted-foreground">
                          <span title={version.first_seen}>{version.first_seen.slice(0, 10)}</span>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </section>
          ))}

      <section className="space-y-2 border-t pt-4">
        <h3 className="text-sm font-medium">{t('diff_title')}</h3>
        {!props.contentAvailable
          ? <p className="text-sm text-muted-foreground">{t('content_unavailable')}</p>
          : (
              <>
                <form
                  className="flex flex-wrap items-end gap-2"
                  onSubmit={(event) => {
                    event.preventDefault();
                    setDiff(null);
                    run('diff', async () => {
                      setDiff(await InferonApi
                        .sameOrigin()
                        .promptDiff({ name: diffName, from, to }));
                    });
                  }}
                >
                  <label className="min-w-40 text-sm">
                    <span className="text-muted-foreground">{t('prompt')}</span>
                    <select
                      className={FIELD_INPUT}
                      value={diffName}
                      onChange={(event) => {
                        setDiffName(event.target.value);
                        setFrom('');
                        setTo('');
                        setDiff(null);
                      }}
                    >
                      <option value="">—</option>
                      {props.prompts.map(entry => (
                        <option key={entry.name} value={entry.name}>{entry.name}</option>
                      ))}
                    </select>
                  </label>
                  {(['from', 'to'] as const).map(side => (
                    <label key={side} className="min-w-40 text-sm">
                      <span className="text-muted-foreground">{t(side)}</span>
                      <select
                        className={FIELD_INPUT}
                        value={side === 'from' ? from : to}
                        onChange={event => (side === 'from' ? setFrom : setTo)(event.target.value)}
                      >
                        <option value="">—</option>
                        {versions.map(version => (
                          <option key={version.prompt_hash} value={version.prompt_hash}>
                            {short(version.prompt_hash)}
                            {' · '}
                            {version.first_seen.slice(0, 10)}
                          </option>
                        ))}
                      </select>
                    </label>
                  ))}
                  <Button type="submit" disabled={busy === 'diff' || !diffName || !from || !to}>
                    {t('diff_action')}
                  </Button>
                </form>

                {diff && (
                  <pre className="
                    mt-2 max-h-96 overflow-auto rounded-md border bg-muted/40
                    p-3 font-mono text-xs
                  "
                  >
                    {withIds(diff.changes).map(change => (
                      <div
                        key={change.id}
                        className={DIFF_CLASS[change.kind]}
                      >
                        {DIFF_SIGN[change.kind]}
                        {' '}
                        {change.text}
                      </div>
                    ))}
                  </pre>
                )}
              </>
            )}
      </section>
    </div>
  );
};
