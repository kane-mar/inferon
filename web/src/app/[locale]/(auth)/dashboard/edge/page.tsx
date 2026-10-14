import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { EdgeNodesPanel } from '@/features/dashboard/EdgeNodesPanel';
import { PageHeader } from '@/features/shell/PageHeader';
import { InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

/**
 * Edge nodes (ITEM-065): the cloud control plane for the hybrid data plane.
 * Inference stays on the customer's network; only small config/telemetry
 * traffic leaves it.
 */
export default async function EdgePage(props: {
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({ locale, namespace: 'EdgePage' });
  const data = await InferonApi.fromEnv().edgeNodes(session.cookie);

  return (
    <>
      <PageHeader title={t('title_bar')} description={t('title_bar_description')} />
      <Card title={t('nodes')}>
        <EdgeNodesPanel data={data} />
      </Card>
    </>
  );
}
