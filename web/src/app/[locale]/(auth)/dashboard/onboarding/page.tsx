import type { OnboardingLabels } from '@/features/onboarding/Onboarding';
import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Onboarding } from '@/features/onboarding/Onboarding';
import { DEFAULT_MODEL } from '@/features/onboarding/snippets';
import { InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';
import { getBaseUrl } from '@/utils/Helpers';

type OnboardingPageProps = {
  params: Promise<{ locale: string }>;
};

/**
 * Guided first-run (ITEM-048): mint the first key, copy the base URL + a
 * client snippet, and watch for the first request. The backend renders no HTML
 * and mints no key — this page is the only first-key surface.
 */
export default async function OnboardingPage(props: OnboardingPageProps) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({ locale, namespace: 'OnboardingPage' });
  const api = InferonApi.fromEnv();
  const [summary, providers] = await Promise.all([
    api.summary(30, 0, session.cookie),
    api.providers(session.cookie),
  ]);
  // AC4: the snippet uses the tenant's enabled/default model so it works for a
  // shared-key tenant and one with their own provider (free-form ids included).
  const models = providers.providers.flatMap(provider => provider.models);
  const enabledModels = models.filter(model => model.enabled).map(model => model.id);
  const defaultModel
    = models.find(model => model.is_default && model.enabled)?.id
      ?? enabledModels[0]
      ?? DEFAULT_MODEL;

  const labels: OnboardingLabels = {
    keyStep: t('key_step'),
    keyStepHelp: t('key_step_help'),
    createKey: t('create_key'),
    creating: t('creating'),
    createKeyFailed: t('create_key_failed'),
    keyOnce: t('key_once'),
    copy: t('copy'),
    copied: t('copied'),
    connectStep: t('connect_step'),
    baseUrlLabel: t('base_url'),
    keyLabel: t('key'),
    modelLabel: t('model_label'),
    modelHelp: t('model_help'),
    linksHelp: t('links_help'),
    providersLink: t('providers_link'),
    requestStep: t('request_step'),
    waiting: t('waiting'),
    received: t('received'),
    goToDashboard: t('go_to_dashboard'),
  };

  return (
    <>
      <div className="mb-6">
        <h1 className="text-2xl font-semibold">{t('title')}</h1>
        <p className="mt-1 text-sm text-muted-foreground">{t('description')}</p>
      </div>

      <Onboarding
        firstTraceAt={summary.first_trace_at}
        appUrl={getBaseUrl()}
        models={enabledModels}
        defaultModel={defaultModel}
        labels={labels}
      />
    </>
  );
}

export const dynamic = 'force-dynamic';
