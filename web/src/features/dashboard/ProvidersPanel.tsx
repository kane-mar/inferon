// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

'use client';

import type { ProviderMetadata, ProviderRow, ProvidersResponse, ProviderTest } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { errorMessage } from '@/libs/errors';
import { InferonApi } from '@/libs/InferonApi';
import { cn } from '@/utils/Helpers';
import { FIELD_INPUT, MONO_FIELD_INPUT } from './formStyles';
import { ProviderModels } from './ProviderModels';

/** Convenience defaults so a tenant only has to paste a key. */
const DEFAULT_BASE: Record<string, string> = {
  'deepseek': 'https://api.deepseek.com/v1',
  'openai': 'https://api.openai.com/v1',
  'openai-compatible': '',
  'anthropic': 'https://api.anthropic.com/v1',
  // A local endpoint is the tenant's own address — nothing sensible to prefill.
  'local': '',
};

/** Base-URL placeholder per kind: the shape we expect the tenant to paste. */
const BASE_PLACEHOLDER: Record<string, string> = {
  local: 'http://192.168.1.10:11434/v1',
};

/** The add form shows the local-specific explanation only for this kind. */
const LOCAL_KIND = 'local';

/**
 * A distinct look per provider kind, so a list of providers is scannable at a
 * glance: a coloured monogram plus a matching badge. Classes are literal so
 * Tailwind keeps them.
 */
const KIND_STYLE: Record<string, { initials: string; monogram: string; badge: string }> = {
  'deepseek': {
    initials: 'DS',
    monogram: 'bg-indigo-500/15 text-indigo-600 dark:text-indigo-300',
    badge: 'bg-indigo-500/10 text-indigo-600 dark:text-indigo-300',
  },
  'openai': {
    initials: 'OA',
    monogram: 'bg-success/15 text-success',
    badge: 'bg-success/10 text-success',
  },
  'anthropic': {
    initials: 'AN',
    monogram: 'bg-warning/15 text-warning',
    badge: 'bg-warning/10 text-warning',
  },
  'local': {
    initials: 'LM',
    monogram: 'bg-teal-500/15 text-teal-600 dark:text-teal-300',
    badge: 'bg-teal-500/10 text-teal-600 dark:text-teal-300',
  },
  'openai-compatible': {
    initials: '··',
    monogram: 'bg-slate-500/15 text-slate-600 dark:text-slate-300',
    badge: 'bg-slate-500/10 text-slate-600 dark:text-slate-300',
  },
};

const openaiCompatible = KIND_STYLE['openai-compatible']!;
const kindStyle = (kind: string) => KIND_STYLE[kind] ?? openaiCompatible;

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

const Chip = (props: { children: React.ReactNode; className?: string; title?: string }) => (
  <span
    title={props.title}
    className={cn('rounded-sm px-1.5 py-0.5 font-mono text-xs', props.className)}
  >
    {props.children}
  </span>
);

/** The provider's kind, with its colour and monogram. */
const KindMonogram = (props: { kind: string }) => {
  const style = kindStyle(props.kind);
  return (
    <span
      aria-hidden
      className={cn(
        `
          flex size-10 shrink-0 items-center justify-center rounded-lg font-mono
          text-xs font-semibold
        `,
        style.monogram,
      )}
    >
      {style.initials}
    </span>
  );
};

type HealthState = 'ok' | 'fail' | 'unknown' | 'pending';

const HealthPill = (props: { state: HealthState; title?: string }) => {
  const t = useTranslations('ProvidersPage');
  const tone: Record<HealthState, string> = {
    ok: 'text-success',
    fail: 'text-destructive',
    unknown: 'text-muted-foreground',
    pending: 'text-muted-foreground',
  };
  const label = props.state === 'ok'
    ? t('connected')
    : props.state === 'fail'
      ? t('unreachable')
      : props.state === 'pending'
        ? t('testing')
        : t('not_checked');
  return (
    <Chip className={tone[props.state]} title={props.title}>
      {label}
    </Chip>
  );
};

const healthOf = (result: ProviderTest | 'pending' | undefined, lastCheckOk: boolean | null): HealthState => {
  if (result === 'pending') {
    return 'pending';
  }
  if (result) {
    return result.ok ? 'ok' : 'fail';
  }
  if (lastCheckOk == null) {
    return 'unknown';
  }
  return lastCheckOk ? 'ok' : 'fail';
};

export const ProvidersPanel = (props: {
  data: ProvidersResponse;
  role: 'owner' | 'member';
}) => {
  const t = useTranslations('ProvidersPage');
  const router = useRouter();
  const kinds = props.data.kinds.length > 0 ? props.data.kinds : ['deepseek'];

  const [showAdd, setShowAdd] = useState(false);
  const [kind, setKind] = useState(kinds[0] as string);
  const [name, setName] = useState('');
  const [baseUrl, setBaseUrl] = useState(DEFAULT_BASE[kinds[0] as string] ?? '');
  const [key, setKey] = useState('');
  const [org, setOrg] = useState('');
  const [headers, setHeaders] = useState('');
  const [probe, setProbe] = useState<{ status: 'testing' | 'ok' | 'error'; message: string } | null>(null);
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
      setShowAdd(false);
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('error')));
    } finally {
      setBusy(false);
    }
  };

  const testDraft = async () => {
    setProbe({ status: 'testing', message: '' });
    try {
      const result = await InferonApi.sameOrigin().testProviderDraft({
        kind,
        base_url: baseUrl.trim(),
        key: key.trim(),
        metadata: metadataFrom(org, headers),
      });
      setProbe(
        result.ok
          ? { status: 'ok', message: t('probe_ok', { count: result.models.length }) }
          : { status: 'error', message: result.error ?? t('probe_failed') },
      );
    } catch (cause) {
      setProbe({ status: 'error', message: errorMessage(cause, t('probe_failed')) });
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
        [providerId]: { ok: false, status: null, error: t('probe_failed') },
      }));
    }
  };

  return (
    <div className="space-y-4">
      {error && <p className="text-sm text-destructive">{error}</p>}

      <div className="flex items-center justify-between gap-3">
        <h2 className="text-sm font-semibold text-muted-foreground">
          {t('list_title', { count: props.data.providers.length })}
        </h2>
        {canEdit && (
          <Button
            size="sm"
            variant={showAdd ? 'outline' : 'default'}
            onClick={() => setShowAdd(current => !current)}
          >
            {showAdd ? t('cancel') : `+ ${t('add_title')}`}
          </Button>
        )}
      </div>

      {canEdit && showAdd && (
        <form
          onSubmit={add}
          className="
            grid gap-3 rounded-lg border bg-card p-4
            sm:grid-cols-2
          "
        >
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
              placeholder={BASE_PLACEHOLDER[kind] ?? 'https://api.deepseek.com/v1'}
              required
              onChange={event => setBaseUrl(event.target.value)}
            />
          </label>

          {kind === LOCAL_KIND && (
            <span className="
              block text-xs text-muted-foreground
              sm:col-span-2
            "
            >
              {t('local_hint')}
            </span>
          )}

          <label className="text-sm">
            <span className="text-muted-foreground">{t('key')}</span>
            <input
              value={key}
              type="password"
              className={MONO_FIELD_INPUT}
              placeholder={kind === LOCAL_KIND ? t('key_optional') : 'sk-…'}
              required={kind !== LOCAL_KIND}
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
              {t('headers_hint')}
            </span>
          </label>

          <span className="
            block text-xs text-muted-foreground
            sm:col-span-2
          "
          >
            {t('key_hint')}
          </span>

          <div className="
            flex flex-wrap items-center gap-3
            sm:col-span-2
          "
          >
            <Button type="submit" disabled={busy}>
              {busy ? t('adding') : t('add')}
            </Button>
            <Button
              type="button"
              variant="outline"
              disabled={busy || probe?.status === 'testing' || !baseUrl.trim() || !key.trim()}
              onClick={testDraft}
            >
              {probe?.status === 'testing' ? t('testing') : t('test_connection')}
            </Button>
            {probe && probe.status !== 'testing' && (
              <span
                className={cn(
                  'text-sm',
                  probe.status === 'ok'
                    ? `text-success`
                    : 'text-destructive',
                )}
              >
                {probe.message}
              </span>
            )}
          </div>
        </form>
      )}

      {props.data.providers.length === 0
        ? (
            <p className="text-sm text-muted-foreground">{t('empty')}</p>
          )
        : (
            <ul className="space-y-3">
              {props.data.providers.map((provider) => {
                const result = tested[provider.id];
                const style = kindStyle(provider.kind);
                const updated = formatDate(provider.updated_at);

                if (editing === provider.id) {
                  return (
                    <li
                      key={provider.id}
                      className={cn(
                        'rounded-lg border bg-card p-4',
                        'border-l-4',
                        style.monogram,
                      )}
                    >
                      <form
                        onSubmit={event => saveEdit(event, provider.id)}
                        className="
                          grid gap-3
                          sm:grid-cols-2
                        "
                      >
                        <h3 className="
                          text-sm font-semibold
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
                            {t('rotate_key_hint')}
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
                            {t('headers_hint')}
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
                    className={cn(
                      'rounded-lg border bg-card',
                      'border-l-4',
                      style.monogram,
                    )}
                  >
                    <div className="flex items-start gap-3 p-4">
                      <KindMonogram kind={provider.kind} />
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="font-medium">{provider.name}</span>
                          <Chip className={style.badge}>{provider.kind}</Chip>
                          {provider.is_default && (
                            <Chip className="bg-accent text-accent-foreground">
                              {t('default')}
                            </Chip>
                          )}
                          <HealthPill
                            state={healthOf(result, provider.last_check_ok)}
                            title={provider.last_check_at ?? undefined}
                          />
                        </div>
                        <div className="
                          mt-1 flex flex-wrap gap-x-3 gap-y-1 text-xs
                          text-muted-foreground
                        "
                        >
                          <span className="truncate font-mono">{provider.base_url}</span>
                          <span className="font-mono">{provider.key_hint}</span>
                          {updated && <span>{t('updated', { date: updated })}</span>}
                        </div>
                      </div>
                    </div>

                    <div className="
                      flex flex-wrap items-center gap-1 border-t px-3 py-2
                    "
                    >
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
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() =>
                          setModelsOpen(current => ({
                            ...current,
                            [provider.id]: !current[provider.id],
                          }))}
                      >
                        {modelsOpen[provider.id] ? t('hide_models') : t('show_models')}
                      </Button>
                      {canEdit && (
                        <Button
                          variant="ghost"
                          size="sm"
                          className="ml-auto text-destructive"
                          disabled={busy}
                          onClick={() => remove(provider.id)}
                        >
                          {t('remove')}
                        </Button>
                      )}
                    </div>

                    {result && result !== 'pending' && (
                      <div
                        className={cn(
                          'border-t px-4 py-2 text-xs',
                          result.ok
                            ? `text-success`
                            : 'text-destructive',
                        )}
                      >
                        <span className="font-medium">
                          {t('test_result')}
                          :
                          {' '}
                        </span>
                        {result.ok
                          ? t('test_ok', { count: result.models ?? 0 })
                          : t('test_failed', { error: result.error ?? t('unreachable') })}
                      </div>
                    )}

                    {modelsOpen[provider.id] && (
                      <div className="border-t p-4">
                        <ProviderModels providerId={provider.id} canEdit={canEdit} />
                      </div>
                    )}
                  </li>
                );
              })}
            </ul>
          )}

      {!canEdit && (
        <p className="text-sm text-muted-foreground">{t('owner_only')}</p>
      )}
    </div>
  );
};
