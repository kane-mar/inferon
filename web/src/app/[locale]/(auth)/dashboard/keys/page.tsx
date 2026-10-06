import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { KeysPanel } from '@/features/dashboard/KeysPanel';
import { PageHeader } from '@/features/shell/PageHeader';
import { InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

type KeysPageProps = {
  params: Promise<{ locale: string }>;
};

export default async function KeysPage(props: KeysPageProps) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({
    locale,
    namespace: 'KeysPage',
  });
  const data = await InferonApi.fromEnv().keys(session.cookie);

  return (
    <>
      <PageHeader
        title={t('title_bar')}
        description={t('title_bar_description')}
      />
      <Card title={t('keys')}>
        <KeysPanel data={data} />
      </Card>
    </>
  );
}
