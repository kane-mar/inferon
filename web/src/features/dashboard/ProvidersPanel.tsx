'use client';

import type { ProviderMetadata, ProviderRow, ProvidersResponse, ProviderTest } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { errorMessage } from '@/libs/errors';
import { InferonApi } from '@/libs/InferonApi';
import { FIELD_INPUT, MONO_FIELD_INPUT } from './formStyles';
import { ProviderModels } from './ProviderModels';

/** Convenience defaults so a tenant only has to paste a key. */
const DEFAULT_BASE: Record<string, string> = {
  'deepseek': 'https://api.deepseek.com/v1',
  'openai': 'https://api.openai.com/v1',
  'openai-compatible': '',
};

/** `Name: Value` per line → metadata headers; blank/malformed lines ignored. */
const parseHeaders = (text: string): Record<string, string> => {
  const headers: Record<string, string> = {};
  for (const line of text.split('\n')) {
    const colon = line.indexOf(':');
    if (colon === -1) {
      continue;
    }
    const name = line.slice(0, colon).trim();
    const value = line.slice(colon + 1).trim();
    if (name) {
      headers[name] = value;
    }
  }
  return headers;
};

const formatHeaders = (headers?: Record<string, string>): string =>
  Object.entries(headers ?? {})
    .map(([name, value]) => `${name}: ${value}`)
    .join('\n');

const metadataFrom = (org: string, headersText: string): ProviderMetadata | undefined => {
  const trimmedOrg = org.trim();
  const headers = parseHeaders(headersText);
  const hasHeaders = Object.keys(headers).length > 0;
  if (!trimmedOrg && !hasHeaders) {
    return undefined;
  }
  return {
    ...(trimmedOrg ? { org: trimmedOrg } : {}),
    ...(hasHeaders ? { headers } : {}),
  };
};

const formatDate = (value: string): string => {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? '' : date.toLocaleDateString();
};

const EMPTY_DRAFT = { name: '', base_url: '', key: '', org: '', headers: '' };

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
  const [org, setOrg] = useState('');
  const [headers, setHeaders] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [tested, setTested] = useState<Record<string, ProviderTest | 'pending'>>({});
  const [editing, setEditing] = useState<string | null>(null);
  const [modelsOpen, setModelsOpen] = useState<Record<string, boolean>>({});
  const [draft, setDraft] = useState(EMPTY_DRAFT);

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
        metadata: metadataFrom(org, headers),
      });
      setName('');
      setKey('');
      setOrg('');
      setHeaders('');
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('error')));
    } finally {
      setBusy(false);
    }
  };

  const setDefault = async (providerId: string) => {
    setBusy(true);
    setError(null);
    try {
      await InferonApi.sameOrigin().setDefaultProvider(providerId);
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('error')));
    } finally {
      setBusy(false);
    }
  };

  const startEdit = (provider: ProviderRow) => {
    setEditing(provider.id);
    setDraft({
      name: provider.name,
      base_url: provider.base_url,
      key: '',
      org: provider.metadata.org ?? '',
      headers: formatHeaders(provider.metadata.headers),
    });
  };

  const saveEdit = async (event: React.FormEvent, providerId: string) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await InferonApi.sameOrigin().updateProvider(providerId, {
        name: draft.name.trim() || undefined,
        base_url: draft.base_url.trim() || undefined,
        key: draft.key.trim() || undefined,
        metadata: metadataFrom(draft.org, draft.headers) ?? {},
      });
      setEditing(null);
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

  const test = async (providerId: string) => {
    setTested(previous => ({ ...previous, [providerId]: 'pending' }));
    try {
      const result = await InferonApi.sameOrigin().testProvider(providerId);
      setTested(previous => ({ ...previous, [providerId]: result }));
    } catch {
      setTested(previous => ({
        ...previous,
        [providerId]: { ok: false, status: null },
      }));
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
              props.data.providers.map((provider) => {
                const result = tested[provider.id];
                if (editing === provider.id) {
                  return (
                    <li
                      key={provider.id}
                      className="
                        border-b p-4
                        last:border-0
                      "
                    >
                      <form
                        onSubmit={event => saveEdit(event, provider.id)}
                        className="
                          grid gap-3
                          sm:grid-cols-2
                        "
                      >
                        <h3 className="
                          font-mono text-xs font-medium tracking-wide uppercase
                          sm:col-span-2
                        "
                        >
                          {t('edit_title')}
                        </h3>
                        <label className="text-sm">
                          <span className="text-muted-foreground">{t('name')}</span>
                          <input
                            value={draft.name}
                            className={FIELD_INPUT}
                            onChange={event => setDraft(current => ({ ...current, name: event.target.value }))}
                          />
                        </label>
                        <label className="text-sm">
                          <span className="text-muted-foreground">{t('base_url')}</span>
                          <input
                            value={draft.base_url}
                            className={MONO_FIELD_INPUT}
                            required
                            onChange={event => setDraft(current => ({ ...current, base_url: event.target.value }))}
                          />
                        </label>
                        <label className="text-sm">
                          <span className="text-muted-foreground">{t('rotate_key')}</span>
                          <input
                            value={draft.key}
                            type="password"
                            className={MONO_FIELD_INPUT}
                            placeholder={provider.key_hint}
                            autoComplete="off"
                            onChange={event => setDraft(current => ({ ...current, key: event.target.value }))}
                          />
                          <span className="
                            mt-1 block text-xs text-muted-foreground
                          "
                          >
                            {t('rotate_key_note')}
                          </span>
                        </label>
                        <label className="text-sm">
                          <span className="text-muted-foreground">{t('org')}</span>
                          <input
                            value={draft.org}
                            className={MONO_FIELD_INPUT}
                            onChange={event => setDraft(current => ({ ...current, org: event.target.value }))}
                          />
                        </label>
                        <label className="
                          text-sm
                          sm:col-span-2
                        "
                        >
                          <span className="text-muted-foreground">{t('headers')}</span>
                          <textarea
                            value={draft.headers}
                            rows={2}
                            className={MONO_FIELD_INPUT}
                            placeholder="anthropic-version: 2023-06-01"
                            onChange={event => setDraft(current => ({ ...current, headers: event.target.value }))}
                          />
                          <span className="
                            mt-1 block text-xs text-muted-foreground
                          "
                          >
                            {t('headers_note')}
                          </span>
                        </label>
                        <div className="
                          flex items-center gap-3
                          sm:col-span-2
                        "
                        >
                          <Button type="submit" disabled={busy}>
                            {busy ? t('saving') : t('save')}
                          </Button>
                          <Button
                            type="button"
                            variant="ghost"
                            disabled={busy}
                            onClick={() => setEditing(null)}
                          >
                            {t('cancel')}
                          </Button>
                        </div>
                      </form>
                    </li>
                  );
                }
                return (
                  <li
                    key={provider.id}
                    className="
                      border-b
                      last:border-0
                    "
                  >
                    <div className="
                      flex flex-wrap items-center gap-3 px-4 py-3 text-sm
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
                      {provider.is_default && (
                        <span className="
                          rounded-sm bg-accent px-1.5 py-0.5 font-mono text-xs
                          text-accent-foreground
                        "
                        >
                          {t('default')}
                        </span>
                      )}
                      <span className="font-mono text-xs text-muted-foreground">
                        {provider.key_hint}
                      </span>
                      {formatDate(provider.updated_at) && (
                        <span className="text-xs text-muted-foreground">
                          {t('updated', { date: formatDate(provider.updated_at) })}
                        </span>
                      )}
                      {result && result !== 'pending' && (
                        <span
                          className={`
                            font-mono text-xs
                            ${
                        result.ok
                          ? `
                            text-emerald-600
                            dark:text-emerald-400
                          `
                          : 'text-destructive'
                        }
                          `}
                        >
                          {result.ok
                            ? t('connected')
                            : result.status == null
                              ? t('unreachable')
                              : `${t('failed')} (${result.status})`}
                        </span>
                      )}
                      {canEdit && (
                        <Button
                          variant="ghost"
                          size="sm"
                          disabled={result === 'pending'}
                          onClick={() => test(provider.id)}
                        >
                          {result === 'pending' ? t('testing') : t('test')}
                        </Button>
                      )}
                      {canEdit && !provider.is_default && (
                        <Button
                          variant="ghost"
                          size="sm"
                          disabled={busy}
                          onClick={() => setDefault(provider.id)}
                        >
                          {t('make_default')}
                        </Button>
                      )}
                      {canEdit && (
                        <Button
                          variant="ghost"
                          size="sm"
                          disabled={busy}
                          onClick={() => startEdit(provider)}
                        >
                          {t('edit')}
                        </Button>
                      )}
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
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() =>
                          setModelsOpen(current => ({
                            ...current,
                            [provider.id]: !current[provider.id],
                          }))}
                      >
                        {modelsOpen[provider.id]
                          ? t('hide_models')
                          : t('models_count', { count: provider.models.length })}
                      </Button>
                    </div>
                    {modelsOpen[provider.id] && (
                      <ProviderModels
                        providerId={provider.id}
                        models={provider.models}
                        canEdit={canEdit}
                      />
                    )}
                  </li>
                );
              })
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
                  className={FIELD_INPUT}
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
                  className={FIELD_INPUT}
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
                  className={MONO_FIELD_INPUT}
                  placeholder="https://api.deepseek.com/v1"
                  required
                  onChange={event => setBaseUrl(event.target.value)}
                />
              </label>

              <label className="text-sm">
                <span className="text-muted-foreground">{t('key')}</span>
                <input
                  value={key}
                  type="password"
                  className={MONO_FIELD_INPUT}
                  placeholder="sk-…"
                  required
                  autoComplete="off"
                  onChange={event => setKey(event.target.value)}
                />
              </label>

              <label className="text-sm">
                <span className="text-muted-foreground">{t('org')}</span>
                <input
                  value={org}
                  className={MONO_FIELD_INPUT}
                  placeholder={t('org_placeholder')}
                  onChange={event => setOrg(event.target.value)}
                />
              </label>

              <label className="
                text-sm
                sm:col-span-2
              "
              >
                <span className="text-muted-foreground">{t('headers')}</span>
                <textarea
                  value={headers}
                  rows={2}
                  className={MONO_FIELD_INPUT}
                  placeholder="anthropic-version: 2023-06-01"
                  onChange={event => setHeaders(event.target.value)}
                />
                <span className="mt-1 block text-xs text-muted-foreground">
                  {t('headers_note')}
                </span>
              </label>

              <span className="
                block text-xs text-muted-foreground
                sm:col-span-2
              "
              >
                {t('key_note')}
              </span>

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
