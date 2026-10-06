import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { ModelsPanel } from '@/features/dashboard/ModelsPanel';
import { TitleBar } from '@/features/dashboard/TitleBar';
import { InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

type ModelsPageProps = {
  params: Promise<{ locale: string }>;
};

/**
 * Model catalog & selection (ITEM-044): the models available from the tenant's
 * provider kinds, with enable/disable and one default per kind. Prices come
 * from the backend cost engine, so they match what a request actually bills.
 */
export default async function ModelsPage(props: ModelsPageProps) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({ locale, namespace: 'ModelsPage' });
  const data = await InferonApi.fromEnv().models(session.cookie);

  return (
    <>
      <TitleBar title={t('title_bar')} description={t('title_bar_description')} />
      <Card title={t('models')}>
        <ModelsPanel data={data} />
      </Card>
    </>
  );
}
