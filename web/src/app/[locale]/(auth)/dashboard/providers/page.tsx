import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { ProvidersPanel } from '@/features/dashboard/ProvidersPanel';
import { TitleBar } from '@/features/dashboard/TitleBar';
import { InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

type ProvidersPageProps = {
  params: Promise<{ locale: string }>;
};

/**
 * Bring-your-own-provider (ITEM-043): register an upstream credential so the
 * proxy runs on the tenant's account. Keys are encrypted at rest and only a
 * masked hint is shown; owners add/remove, members view.
 */
export default async function ProvidersPage(props: ProvidersPageProps) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({ locale, namespace: 'ProvidersPage' });
  const data = await InferonApi.fromEnv().providers(session.cookie);

  return (
    <>
      <TitleBar
        title={t('title_bar')}
        description={t('title_bar_description')}
      />
      <Card title={t('providers')}>
        <ProvidersPanel data={data} role={data.role} />
      </Card>
    </>
  );
}
