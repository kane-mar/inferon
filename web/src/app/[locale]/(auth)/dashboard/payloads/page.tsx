import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { KpiCard } from '@/features/dashboard/KpiCard';
import { coverage, integer, rate, usd } from '@/features/dashboard/reportFormat';
import { reportDays } from '@/features/dashboard/reportWindow';
import { WindowChip } from '@/features/dashboard/WindowChip';
import { PageHeader } from '@/features/shell/PageHeader';
import { Link } from '@/libs/I18nNavigation';
import { InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

/** A labelled on/off/known value, used by every card here. */
const State = (props: { label: string; value: string; tone?: 'good' | 'off' }) => (
  <div className="flex items-baseline justify-between gap-3 py-1">
    <span className="text-sm text-muted-foreground">{props.label}</span>
    <span
      className={`
        text-sm font-medium
        ${props.tone === 'good'
    ? `
      text-emerald-700
      dark:text-emerald-400
    `
    : ''}
        ${props.tone === 'off' ? 'text-muted-foreground' : ''}
      `}
    >
      {props.value}
    </span>
  </div>
);

/**
 * Payloads (ITEM-090/091/096/103): what happens to prompts and completions?
 * Whether they are stored and for how long, what the lints found, what pruning
 * removed, and which PII shapes were detected. Counts only — never a value.
 */
export default async function PayloadsPage(props: {
  params: Promise<{ locale: string }>;
  searchParams: Promise<{ days?: string }>;
}) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const days = reportDays(await props.searchParams);
  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({ locale, namespace: 'PayloadsPage' });
  const data = await InferonApi.fromEnv().payloads(days, session.cookie);
  const { capture, lint, pruning, pii } = data;

  return (
    <>
      <PageHeader
        title={t('title_bar')}
        description={t('title_bar_description')}
        action={<WindowChip days={days} />}
      />

      <div className="
        mt-6 grid grid-cols-2 gap-3
        lg:grid-cols-4
      "
      >
        <KpiCard
          label={t('capture_state')}
          icon="store"
          value={capture.enabled ? t('on') : t('off')}
          tone={capture.enabled ? 'good' : undefined}
          hint={capture.enabled
            ? t('retention', { days: capture.retention_days })
            : t('capture_off_help')}
        />
        <KpiCard
          label={t('lint_coverage')}
          icon="lint"
          value={coverage(lint.preflight.scanned, data.requests)}
          window={days}
          definition={t('lint_coverage_help')}
          hint={t('lint_scanned', {
            scanned: integer(lint.preflight.scanned),
            total: integer(data.requests),
          })}
        />
        <KpiCard
          label={t('lint_findings')}
          icon="warning"
          value={integer(lint.preflight.findings)}
          window={days}
          definition={t('lint_findings_help')}
          hint={usd(lint.preflight.wasted_usd)}
        />
        <KpiCard
          label={t('prune_removed')}
          icon="prune"
          value={integer(pruning.rewritten)}
          window={days}
          definition={t('prune_removed_help')}
          hint={pruning.policy?.enabled ? t('prune_on') : t('prune_off')}
        />
      </div>

      <div className="
        mt-4 grid grid-cols-1 items-start gap-4
        lg:grid-cols-2
      "
      >
        {/* Capture and retention */}
        <Card title={t('capture')} icon="store">
          <p className="mb-3 text-xs text-muted-foreground">{t('capture_help')}</p>
          <State
            label={t('storing')}
            value={capture.enabled ? t('on') : t('off')}
            tone={capture.enabled ? 'good' : 'off'}
          />
          <State label={t('retention_label')} value={t('days', { days: capture.retention_days })} />
          <Link
            href="/dashboard/routing"
            className="
              mt-3 inline-block text-sm text-primary underline-offset-4
              hover:underline
            "
          >
            {t('change_in_routing')}
          </Link>
        </Card>

        {/* The lint */}
        <Card title={t('lint')} icon="lint">
          <p className="mb-3 text-xs text-muted-foreground">{t('lint_help')}</p>
          <State
            label={t('lint_state')}
            value={lint.enabled ? t('on') : t('off')}
            tone={lint.enabled ? 'good' : 'off'}
          />
          <State
            label={t('rules')}
            value={lint.rules.length > 0 ? lint.rules.join(' · ') : t('no_rules')}
          />
          <State
            label={t('dismissed')}
            value={integer(lint.preflight.dismissed)}
          />
          <State
            label={t('batch')}
            value={t('batch_values', {
              checked: integer(lint.batch.checked),
              flagged: integer(lint.batch.flagged),
            })}
          />
          {Object.keys(lint.preflight.by_kind).length > 0 && (
            <div className="mt-2 flex flex-wrap gap-2">
              {Object.entries(lint.preflight.by_kind).map(([kind, count]) => (
                <span
                  key={kind}
                  className="
                    rounded-md border px-2 py-1 font-mono text-xs
                    text-muted-foreground
                  "
                >
                  {kind}
                  {' · '}
                  {integer(count)}
                </span>
              ))}
            </div>
          )}
        </Card>

        {/* Pruning */}
        <Card title={t('pruning')} icon="prune">
          <p className="mb-3 text-xs text-muted-foreground">{t('pruning_help')}</p>
          {pruning.policy == null || !pruning.policy.enabled
            ? <p className="text-sm text-muted-foreground">{t('pruning_off')}</p>
            : (
                <>
                  <State label={t('holdout')} value={`${pruning.policy.holdout_percent}%`} />
                  <State
                    label={t('budget')}
                    value={pruning.policy.max_prompt_tokens == null
                      ? t('no_budget')
                      : integer(pruning.policy.max_prompt_tokens)}
                  />
                  <State
                    label={t('pruned_ok')}
                    value={rate(pruning.pruned_ok_rate)}
                    tone="good"
                  />
                  <State label={t('holdout_ok')} value={rate(pruning.holdout_ok_rate)} />
                  {pruning.delta != null && (
                    <State
                      label={t('delta')}
                      value={`${pruning.delta >= 0 ? '+' : ''}${(pruning.delta * 100).toFixed(1)} pts`}
                    />
                  )}
                  <State
                    label={t('saved')}
                    value={`${integer(pruning.tokens_saved_est)} tok · ${usd(pruning.usd_saved_est)}`}
                  />
                </>
              )}
          <Link
            href="/dashboard/routing"
            className="
              mt-3 inline-block text-sm text-primary underline-offset-4
              hover:underline
            "
          >
            {t('change_pruning')}
          </Link>
        </Card>

        {/* PII */}
        <Card title={t('pii')} icon="pii">
          <p className="mb-3 text-xs text-muted-foreground">{t('pii_help')}</p>
          <State
            label={t('detection')}
            value={pii.detection ? t('on') : t('off')}
            tone={pii.detection ? 'good' : 'off'}
          />
          <State
            label={t('redact_storage')}
            value={pii.redact_storage ? t('on') : t('off')}
            tone={pii.redact_storage ? 'good' : 'off'}
          />
          <State
            label={t('redact_wire')}
            value={pii.redact_wire ? t('on') : t('off')}
            tone={pii.redact_wire ? 'good' : 'off'}
          />
          <State
            label={t('matches')}
            value={t('matches_value', {
              matches: integer(pii.matches),
              scanned: integer(pii.scanned),
            })}
          />
          {Object.keys(pii.by_class).length > 0 && (
            <div className="mt-2 flex flex-wrap gap-2">
              {Object.entries(pii.by_class).map(([piiClass, count]) => (
                <span
                  key={piiClass}
                  className="
                    rounded-md border px-2 py-1 font-mono text-xs
                    text-muted-foreground
                  "
                >
                  {piiClass}
                  {' · '}
                  {integer(count)}
                </span>
              ))}
            </div>
          )}
        </Card>
      </div>
    </>
  );
}

export const dynamic = 'force-dynamic';
