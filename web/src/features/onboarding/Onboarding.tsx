// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

'use client';

import type { SnippetLang } from './snippets';
import { useEffect, useState } from 'react';
import { buttonVariants } from '@/components/ui/buttonVariants';
import { Link } from '@/libs/I18nNavigation';
import { InferonApi } from '@/libs/InferonApi';
import { cn } from '@/utils/Helpers';
import {
  buildSnippet,
  SNIPPET_LANGS,
  snippetFile,

} from './snippets';

export type OnboardingLabels = {
  keyStep: string;
  keyStepHelp: string;
  createKey: string;
  creating: string;
  createKeyFailed: string;
  keyOnce: string;
  copy: string;
  copied: string;
  connectStep: string;
  baseUrlLabel: string;
  keyLabel: string;
  modelLabel: string;
  modelHelp: string;
  linksHelp: string;
  providersLink: string;
  requestStep: string;
  waiting: string;
  received: string;
  goToDashboard: string;
};

const LANG_LABEL: Record<SnippetLang, string> = {
  python: 'Python',
  node: 'Node',
  curl: 'cURL',
};

const StepHeading = (props: { n: number; children: React.ReactNode }) => (
  <h2 className="flex items-center gap-2 text-sm font-semibold">
    <span className="
      flex size-6 items-center justify-center rounded-full bg-primary font-mono
      text-xs text-primary-foreground
    "
    >
      {props.n}
    </span>
    {props.children}
  </h2>
);

export const Onboarding = (props: {
  firstTraceAt: string | null;
  appUrl: string;
  /** The tenant's enabled catalog models (may be empty for a free-form provider). */
  models: string[];
  defaultModel: string;
  labels: OnboardingLabels;
}) => {
  const { labels } = props;
  const [key, setKey] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const [lang, setLang] = useState<SnippetLang>('python');
  const [model, setModel] = useState(props.defaultModel);
  const [copied, setCopied] = useState<string | null>(null);
  const [connected, setConnected] = useState(props.firstTraceAt != null);

  const copy = async (value: string, id: string) => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(id);
      setTimeout(setCopied, 1500, null);
    } catch {
      // clipboard unavailable (insecure context) — no-op
    }
  };

  const mint = async () => {
    setBusy(true);
    setFailed(false);
    try {
      const created = await InferonApi.sameOrigin().createKey('first');
      setKey(created.key);
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  };

  // Poll for the tenant's first trace and flip the whole page to "connected".
  useEffect(() => {
    if (connected) {
      return;
    }
    const timer = setInterval(async () => {
      try {
        const summary = await InferonApi.sameOrigin().summary(30);
        if (summary.first_trace_at) {
          setConnected(true);
        }
      } catch {
        // keep waiting
      }
    }, 5000);
    return () => clearInterval(timer);
  }, [connected]);

  const apiBase = `${props.appUrl.replace(/\/+$/, '')}/v1`;
  const apiKey = key ?? 'sk-inferon-\u2026';
  const snippet = buildSnippet(lang, props.appUrl, apiKey, model);

  return (
    <div className="mx-auto max-w-2xl space-y-4">
      <div
        className={cn(
          'flex items-center gap-2 rounded-lg border px-4 py-2.5 text-sm',
          connected
            ? 'border-success/30 bg-success/10 text-success'
            : 'text-muted-foreground',
        )}
      >
        <span aria-hidden>{connected ? '✓' : '⋯'}</span>
        {connected ? labels.received : labels.waiting}
      </div>

      {/* 1 — key */}
      <section className="space-y-3 rounded-lg border bg-card p-5">
        <StepHeading n={1}>{labels.keyStep}</StepHeading>

        {key
          ? (
              <div className="space-y-2">
                <p className="text-sm text-muted-foreground">{labels.keyOnce}</p>
                <div className="flex items-center gap-2">
                  <code className="
                    min-w-0 flex-1 truncate rounded-md border bg-muted px-3 py-2
                    font-mono text-xs
                  "
                  >
                    {key}
                  </code>
                  <button
                    type="button"
                    className={buttonVariants({ variant: 'outline', size: 'sm' })}
                    onClick={() => copy(key, 'key')}
                  >
                    {copied === 'key' ? labels.copied : labels.copy}
                  </button>
                </div>
              </div>
            )
          : (
              <>
                <p className="text-sm text-muted-foreground">{labels.keyStepHelp}</p>
                <button
                  type="button"
                  className={buttonVariants()}
                  disabled={busy}
                  onClick={mint}
                >
                  {busy ? labels.creating : labels.createKey}
                </button>
                {failed && (
                  <p className="text-sm text-destructive">{labels.createKeyFailed}</p>
                )}
              </>
            )}
      </section>

      {/* 2 — connect */}
      <section className="space-y-3 rounded-lg border bg-card p-5">
        <StepHeading n={2}>{labels.connectStep}</StepHeading>

        <dl className="space-y-2 text-sm">
          <div className="flex items-center gap-2">
            <dt className="w-24 shrink-0 text-muted-foreground">{labels.baseUrlLabel}</dt>
            <dd className="min-w-0 flex-1">
              <code className="
                inline-block max-w-full truncate rounded-md border bg-muted px-2
                py-1 font-mono text-xs
              "
              >
                {apiBase}
              </code>
            </dd>
          </div>
          <div className="flex items-center gap-2">
            <dt className="w-24 shrink-0 text-muted-foreground">{labels.keyLabel}</dt>
            <dd className="min-w-0 flex-1">
              <code className="
                inline-block max-w-full truncate rounded-md border bg-muted px-2
                py-1 font-mono text-xs
              "
              >
                {apiKey}
              </code>
            </dd>
          </div>
        </dl>

        <label className="block text-sm">
          <span className="text-muted-foreground">{labels.modelLabel}</span>
          <input
            value={model}
            list="onboarding-models"
            className="
              mt-1 w-full rounded-md border bg-background px-3 py-1.5 font-mono
              text-xs outline-none
              focus-visible:border-ring focus-visible:ring-3
              focus-visible:ring-ring/50
            "
            onChange={event => setModel(event.target.value)}
          />
          <datalist id="onboarding-models">
            {props.models.map(item => <option key={item} value={item} />)}
          </datalist>
          <span className="mt-1 block text-xs text-muted-foreground">
            {labels.modelHelp}
          </span>
        </label>

        <p className="text-xs text-muted-foreground">
          {labels.linksHelp}
          {' '}
          <Link href="/dashboard/providers" className="underline">
            {labels.providersLink}
          </Link>
        </p>

        <div className="overflow-hidden rounded-lg border bg-[#1e1e1e]">
          <div className="
            flex items-center justify-between border-b border-white/10 px-3 py-2
          "
          >
            <div className="flex gap-1">
              {SNIPPET_LANGS.map(item => (
                <button
                  key={item}
                  type="button"
                  onClick={() => setLang(item)}
                  className={`
                    rounded-sm px-2 py-0.5 font-mono text-xs
                    ${
                item === lang
                  ? 'bg-white/15 text-white'
                  : `
                    text-white/50
                    hover:text-white/80
                  `
                }
                  `}
                >
                  {LANG_LABEL[item]}
                </button>
              ))}
            </div>
            <div className="flex items-center gap-2">
              <span className="font-mono text-xs text-white/40">{snippetFile(lang)}</span>
              <button
                type="button"
                onClick={() => copy(snippet, 'snippet')}
                className="
                  rounded-sm px-2 py-0.5 font-mono text-xs text-white/50
                  hover:text-white/80
                "
              >
                {copied === 'snippet' ? labels.copied : labels.copy}
              </button>
            </div>
          </div>
          <pre className="
            overflow-x-auto p-4 font-mono text-xs/relaxed text-white/90
          "
          >
            <code>{snippet}</code>
          </pre>
        </div>
      </section>

      {/* 3 — first request */}
      <section className="space-y-3 rounded-lg border bg-card p-5">
        <StepHeading n={3}>{labels.requestStep}</StepHeading>
        <Link href="/dashboard" className={buttonVariants({ variant: 'outline' })}>
          {labels.goToDashboard}
        </Link>
      </section>
    </div>
  );
};
