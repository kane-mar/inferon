// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import type { OnboardingLabels } from '@/features/onboarding/Onboarding';
import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { KeysPanel } from '@/features/dashboard/KeysPanel';
import { MembersPanel } from '@/features/dashboard/MembersPanel';
import { Onboarding } from '@/features/onboarding/Onboarding';
import { DEFAULT_MODEL } from '@/features/onboarding/snippets';
import { PageHeader } from '@/features/shell/PageHeader';
import { Link } from '@/libs/I18nNavigation';
import { InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';
import { getBaseUrl } from '@/utils/Helpers';

const LINK = `
  text-primary underline-offset-4
  hover:underline
`;

/** One `label · value` line: the shape every settings card uses. */
const Field = (props: { label: string; value: React.ReactNode; mono?: boolean }) => (
  <div className="flex items-baseline justify-between gap-4 py-1.5">
    <dt className="text-sm text-muted-foreground">{props.label}</dt>
    <dd className={props.mono ? 'truncate font-mono text-sm' : 'text-sm'}>
      {props.value}
    </dd>
  </div>
);

/**
 * Settings, on one page: who you are, how to connect a client, the keys that
 * authenticate it, and who else can see the tenant.
 *
 * These used to be four separate pages behind a submenu; the content is the
 * same, reading as one workspace instead of a hub of links.
 */
export default async function SettingsPage(props: {
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({ locale, namespace: 'SettingsPage' });

  const api = InferonApi.fromEnv();
  const [summary, models, keysData, membersData] = await Promise.all([
    api.summary(30, 0, session.cookie),
    api.models(session.cookie),
    api.keys(session.cookie),
    api.members(session.cookie),
  ]);

  // The connect step uses a model this tenant actually serves, so the snippet
  // works for a shared-key tenant and for one with its own provider.
  const available = models.providers.flatMap(provider => provider.models);
  const enabledModels = available.filter(model => model.enabled).map(model => model.id);
  const defaultModel
    = available.find(model => model.is_default && model.enabled)?.id
      ?? enabledModels[0]
      ?? DEFAULT_MODEL;

  // One namespace per page; the panels keep their own for the strings only they
  // use. This page's own copy lives in `SettingsPage`.
  const labels: OnboardingLabels = {
    keyStep: t('connect_key_step'),
    keyStepHelp: t('connect_key_step_help'),
    createKey: t('connect_create_key'),
    creating: t('connect_creating'),
    createKeyFailed: t('connect_create_key_failed'),
    keyOnce: t('connect_key_once'),
    copy: t('connect_copy'),
    copied: t('connect_copied'),
    connectStep: t('connect_step'),
    baseUrlLabel: t('connect_base_url'),
    keyLabel: t('connect_key'),
    modelLabel: t('connect_model_label'),
    modelHelp: t('connect_model_help'),
    linksHelp: t('connect_links_help'),
    providersLink: t('connect_providers_link'),
    requestStep: t('connect_request_step'),
    waiting: t('connect_waiting'),
    received: t('connect_received'),
    goToDashboard: t('connect_go_to_dashboard'),
  };

  return (
    <>
      <PageHeader title={t('title_bar')} />

      <div className="
        mt-6 grid grid-cols-1 items-start gap-4
        lg:grid-cols-3
      "
      >
        <div className="
          space-y-4
          lg:col-span-2
        "
        >
          <Card title={t('connect_title')} icon="cost">
            <p className="mb-4 text-sm text-muted-foreground">{t('connect_body')}</p>
            <Onboarding
              firstTraceAt={summary.first_trace_at}
              appUrl={getBaseUrl()}
              models={enabledModels}
              defaultModel={defaultModel}
              labels={labels}
            />
          </Card>

          <Card title={t('keys_title')} icon="lock">
            <KeysPanel data={keysData} />
          </Card>

          <Card title={t('members_title')} icon="store">
            <MembersPanel data={membersData} />
          </Card>
        </div>

        <div className="space-y-4">
          <Card title={t('workspace_title')} icon="layers">
            <dl className="divide-y">
              <Field label={t('signed_in_as')} value={session.me.github_login} />
              <Field
                label={t('role')}
                value={membersData.role === 'owner' ? t('role_owner') : t('role_member')}
              />
              <Field
                label={t('tenant')}
                value={summary.tenant_id ?? '—'}
                mono
              />
              {session.me.is_operator && (
                <Field label={t('access')} value={t('operator_badge')} />
              )}
            </dl>
          </Card>

          <Card title={t('related_title')} icon="layers">
            <p className="mb-3 text-sm text-muted-foreground">{t('related_body')}</p>
            <ul className="space-y-2 text-sm">
              <li>
                <Link href="/dashboard/providers" className={LINK}>
                  {t('link_providers')}
                </Link>
              </li>
              <li>
                <Link href="/dashboard/routing" className={LINK}>
                  {t('link_routing')}
                </Link>
              </li>
              <li>
                <Link href="/dashboard/audit" className={LINK}>
                  {t('link_audit')}
                </Link>
              </li>
            </ul>
          </Card>
        </div>
      </div>
    </>
  );
}

export const dynamic = 'force-dynamic';
