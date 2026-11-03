import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { PromptsPanel } from '@/features/dashboard/PromptsPanel';
import { PageHeader } from '@/features/shell/PageHeader';
import { InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

/**
 * Prompt registry (ITEM-097): a content hash can be named, its versions are a
 * view over the traces that produced them, and two versions can be diffed.
 * Read-only in v1; deleting a label never deletes a trace.
 */
export default async function PromptsPage(props: {
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({ locale, namespace: 'PromptsPage' });
  const data = await InferonApi.fromEnv().prompts(session.cookie);

  return (
    <>
      <PageHeader title={t('title_bar')} description={t('title_bar_description')} />
      <Card title={t('title_bar')}>
        <PromptsPanel
          prompts={data.prompts}
          canEdit={data.role === 'owner'}
          contentAvailable={data.content_available === true}
        />
      </Card>
    </>
  );
}

export const dynamic = 'force-dynamic';
