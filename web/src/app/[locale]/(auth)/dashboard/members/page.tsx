import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { MembersPanel } from '@/features/dashboard/MembersPanel';
import { PageHeader } from '@/features/shell/PageHeader';
import { InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

type MembersPageProps = {
  params: Promise<{ locale: string }>;
};

export default async function MembersPage(props: MembersPageProps) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({
    locale,
    namespace: 'MembersPage',
  });
  const data = await InferonApi.fromEnv().members(session.cookie);

  return (
    <>
      <PageHeader
        title={t('title_bar')}
        description={t('title_bar_description')}
      />
      <Card title={t('members')}>
        <MembersPanel data={data} />
      </Card>
    </>
  );
}
