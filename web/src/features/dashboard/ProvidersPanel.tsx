'use client';

import type { ProvidersResponse } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { InferonApi, InferonApiError } from '@/libs/InferonApi';

const errorMessage = (error: unknown, fallback: string) =>
  error instanceof InferonApiError ? error.message : fallback;

/** Convenience defaults so a tenant only has to paste a key. */
const DEFAULT_BASE: Record<string, string> = {
  'deepseek': 'https://api.deepseek.com/v1',
  'openai': 'https://api.openai.com/v1',
  'openai-compatible': '',
};

const INPUT = `
  mt-1 w-full rounded-md border bg-background px-3 py-2 text-sm outline-none
  focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50
`;

export const ProvidersPanel = (props: {
  data: ProvidersResponse;
  role: 'owner' | 'member';
}) => {
  const t = useTranslations('ProvidersPage');
  const router = useRouter();
  const kinds = props.data.kinds.length > 0 ? props.data.kinds : ['deepseek'];

  const [kind, setKind] = useState(kinds[0] as string);
  const [name, setName] = useState('');
  const [baseUrl, setBaseUrl] = useState(DEFAULT_BASE[kinds[0] as string] ?? '');
  const [key, setKey] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const canEdit = props.role === 'owner';

  const add = async (event: React.FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await InferonApi.sameOrigin().createProvider({
        kind,
        name: name.trim() || undefined,
        base_url: baseUrl.trim(),
        key: key.trim(),
      });
      setName('');
      setKey('');
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('error')));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (providerId: string) => {
    setBusy(true);
    setError(null);
    try {
      await InferonApi.sameOrigin().removeProvider(providerId);
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('error')));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-6">
      {error && <p className="text-sm text-destructive">{error}</p>}

      <ul className="overflow-hidden rounded-lg border bg-card">
        {props.data.providers.length === 0
          ? (
              <li className="p-4 text-sm text-muted-foreground">{t('empty')}</li>
            )
          : (
              props.data.providers.map(provider => (
                <li
                  key={provider.id}
                  className="
                    flex flex-wrap items-center gap-3 border-b px-4 py-3 text-sm
                    last:border-0
                  "
                >
                  <div className="min-w-0 flex-1">
                    <div className="font-medium">{provider.name}</div>
                    <div className="
                      truncate font-mono text-xs text-muted-foreground
                    "
                    >
                      {provider.base_url}
                    </div>
                  </div>
                  <span className="
                    rounded-sm bg-secondary px-1.5 py-0.5 font-mono text-xs
                  "
                  >
                    {provider.kind}
                  </span>
                  <span className="font-mono text-xs text-muted-foreground">
                    {provider.key_hint}
                  </span>
                  {canEdit && (
                    <Button
                      variant="ghost"
                      size="sm"
                      disabled={busy}
                      onClick={() => remove(provider.id)}
                    >
                      {t('remove')}
                    </Button>
                  )}
                </li>
              ))
            )}
      </ul>

      {canEdit
        ? (
            <form
              onSubmit={add}
              className="
                grid gap-3 rounded-lg border bg-card p-4
                sm:grid-cols-2
              "
            >
              <h2 className="
                font-mono text-xs font-medium tracking-wide uppercase
                sm:col-span-2
              "
              >
                {t('add_title')}
              </h2>

              <label className="text-sm">
                <span className="text-muted-foreground">{t('kind')}</span>
                <select
                  value={kind}
                  className={INPUT}
                  onChange={(event) => {
                    setKind(event.target.value);
                    setBaseUrl(DEFAULT_BASE[event.target.value] ?? '');
                  }}
                >
                  {kinds.map(item => (
                    <option key={item} value={item}>{item}</option>
                  ))}
                </select>
              </label>

              <label className="text-sm">
                <span className="text-muted-foreground">{t('name')}</span>
                <input
                  value={name}
                  className={INPUT}
                  placeholder={t('name_placeholder')}
                  onChange={event => setName(event.target.value)}
                />
              </label>

              <label className="
                text-sm
                sm:col-span-2
              "
              >
                <span className="text-muted-foreground">{t('base_url')}</span>
                <input
                  value={baseUrl}
                  className={`
                    ${INPUT}
                    font-mono
                  `}
                  placeholder="https://api.deepseek.com/v1"
                  required
                  onChange={event => setBaseUrl(event.target.value)}
                />
              </label>

              <label className="
                text-sm
                sm:col-span-2
              "
              >
                <span className="text-muted-foreground">{t('key')}</span>
                <input
                  value={key}
                  type="password"
                  className={`
                    ${INPUT}
                    font-mono
                  `}
                  placeholder="sk-…"
                  required
                  autoComplete="off"
                  onChange={event => setKey(event.target.value)}
                />
                <span className="mt-1 block text-xs text-muted-foreground">
                  {t('key_note')}
                </span>
              </label>

              <div className="sm:col-span-2">
                <Button type="submit" disabled={busy}>
                  {busy ? t('adding') : t('add')}
                </Button>
              </div>
            </form>
          )
        : (
            <p className="text-sm text-muted-foreground">{t('owner_only')}</p>
          )}
    </div>
  );
};
