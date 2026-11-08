// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

'use client';

import type { LintRule, TenantConfig } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { centsToUsd } from '@/libs/dashboardMetrics';
import { errorMessage } from '@/libs/errors';
import { InferonApi } from '@/libs/InferonApi';
import { FIELD_INPUT } from './formStyles';
import { money } from './money';

/** ITEM-090: the pre-flight rules, in reporting order. */
const LINT_RULES: { rule: LintRule; label: string; help: string }[] = [
  {
    rule: 'repeated_system_prompt',
    label: 'Repeated system prompt',
    help: 'The same system prompt again within 10 minutes.',
  },
  {
    rule: 'oversized_schema',
    label: 'Oversized tool schema',
    help: 'Tool/function definitions beyond 8 KB.',
  },
  {
    rule: 'boilerplate',
    label: 'Boilerplate block',
    help: 'A system prompt beyond 8,000 characters.',
  },
  {
    rule: 'duplicate_turns',
    label: 'Duplicate turns',
    help: 'A message that repeats an earlier (role, content) pair exactly.',
  },
];

export const ControlsPanel = (props: { data: TenantConfig; modelIds: string[] }) => {
  const t = useTranslations('ControlsPage');
  const router = useRouter();
  const { data } = props;
  const [budget, setBudget] = useState((data.budget_limit_cents / 100).toFixed(2));
  const [cheap, setCheap] = useState(data.cheap_fallback_model ?? '');
  const [guard, setGuard] = useState(data.guard_enabled);
  const [routerOn, setRouterOn] = useState(data.router_enabled);
  const [threshold, setThreshold] = useState(String(data.router_threshold));
  const [capture, setCapture] = useState(data.capture_payloads);
  const [retention, setRetention] = useState(String(data.payload_retention_days));
  // ITEM-090: the lint is optional per rule, not just on/off.
  const [lintOn, setLintOn] = useState(data.lint_enabled);
  const [lintRules, setLintRules] = useState<LintRule[]>(data.lint_rules);
  const toggleRule = (rule: LintRule) =>
    setLintRules(rules =>
      rules.includes(rule) ? rules.filter(known => known !== rule) : [...rules, rule],
    );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const canEdit = data.role === 'owner';
  const modelIds = props.modelIds;

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setSaved(false);
    try {
      await InferonApi.sameOrigin().updateConfig({
        budget_limit_cents: Math.max(0, Math.round((Number(budget) || 0) * 100)),
        cheap_fallback_model: cheap.trim() || null,
        guard_enabled: guard,
        router_enabled: routerOn,
        router_threshold: Math.min(1, Math.max(0, Number(threshold) || 0)),
        capture_payloads: capture,
        payload_retention_days: Math.max(0, Math.round(Number(retention) || 0)),
        lint_enabled: lintOn,
        lint_rules: lintRules,
      });
      setSaved(true);
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('error')));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={save} className="space-y-5">
      {error && <p className="text-sm text-destructive">{error}</p>}

      <div className="space-y-1">
        <label className="text-sm">
          <span className="text-muted-foreground">{t('budget')}</span>
          <input
            className={FIELD_INPUT}
            inputMode="decimal"
            value={budget}
            disabled={!canEdit}
            onChange={event => setBudget(event.target.value)}
          />
        </label>
        <p className="text-xs text-muted-foreground">
          {data.budget_limit_cents > 0
            ? t('spent_of', {
                spent: money(centsToUsd(data.current_spend_cents)),
                limit: money(centsToUsd(data.budget_limit_cents)),
              })
            : t('spent_unlimited', { spent: money(centsToUsd(data.current_spend_cents)) })}
        </p>
      </div>

      <label className="block text-sm">
        <span className="text-muted-foreground">{t('cheap_fallback')}</span>
        <select
          className={FIELD_INPUT}
          value={cheap}
          disabled={!canEdit}
          onChange={event => setCheap(event.target.value)}
        >
          <option value="">{t('none')}</option>
          {modelIds.map(id => <option key={id} value={id}>{id}</option>)}
        </select>
        <span className="mt-1 block text-xs text-muted-foreground">{t('cheap_help')}</span>
      </label>

      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          className="size-4 accent-primary"
          checked={guard}
          disabled={!canEdit}
          onChange={event => setGuard(event.target.checked)}
        />
        {t('guard')}
      </label>

      <div className="space-y-1">
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            className="size-4 accent-primary"
            checked={capture}
            disabled={!canEdit}
            onChange={event => setCapture(event.target.checked)}
          />
          {t('capture')}
        </label>
        <p className="text-xs text-muted-foreground">{t('capture_help')}</p>
        <label className="text-sm">
          <span className="text-muted-foreground">{t('retention')}</span>
          <input
            className={FIELD_INPUT}
            inputMode="numeric"
            value={retention}
            disabled={!canEdit || !capture}
            onChange={event => setRetention(event.target.value)}
          />
        </label>
        <p className="text-xs text-muted-foreground">{t('retention_help')}</p>
      </div>

      <div className="space-y-1">
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            className="size-4 accent-primary"
            checked={routerOn}
            disabled={!canEdit}
            onChange={event => setRouterOn(event.target.checked)}
          />
          {t('router')}
        </label>
        <label className="block text-sm">
          <span className="text-muted-foreground">{t('threshold')}</span>
          <input
            className={FIELD_INPUT}
            inputMode="decimal"
            value={threshold}
            disabled={!canEdit || !routerOn}
            onChange={event => setThreshold(event.target.value)}
          />
          <span className="mt-1 block text-xs text-muted-foreground">{t('threshold_help')}</span>
        </label>
      </div>

      {/* ITEM-090: which pre-flight checks run. Advisory only — the payload is
          forwarded unchanged — so switching one off costs nothing but the signal. */}
      <fieldset className="space-y-2 border-t pt-4">
        <label className="flex items-center gap-2 text-sm font-medium">
          <input
            type="checkbox"
            className="size-4 accent-primary"
            checked={lintOn}
            disabled={!canEdit}
            onChange={event => setLintOn(event.target.checked)}
          />
          {t('lint')}
        </label>
        <p className="text-xs text-muted-foreground">{t('lint_help')}</p>
        <div className="space-y-1.5 pl-6">
          {LINT_RULES.map(entry => (
            <label key={entry.rule} className="flex items-start gap-2 text-sm">
              <input
                type="checkbox"
                className="mt-0.5 size-4 accent-primary"
                checked={lintRules.includes(entry.rule)}
                disabled={!canEdit || !lintOn}
                onChange={() => toggleRule(entry.rule)}
              />
              <span>
                {entry.label}
                <span className="block text-xs text-muted-foreground">{entry.help}</span>
              </span>
            </label>
          ))}
        </div>
      </fieldset>

      {canEdit
        ? (
            <div className="flex items-center gap-3">
              <Button type="submit" disabled={busy}>
                {busy ? t('saving') : t('save')}
              </Button>
              {saved && (
                <span className="
                  text-sm text-success
                "
                >
                  {t('saved')}
                </span>
              )}
            </div>
          )
        : (
            <p className="text-sm text-muted-foreground">{t('owner_only')}</p>
          )}
    </form>
  );
};
