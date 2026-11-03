import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { EvalsPanel } from '@/features/dashboard/EvalsPanel';
import { PageHeader } from '@/features/shell/PageHeader';
import { InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

/**
 * Evals (ITEM-098): pin a gold set from verified traffic, run a model against
 * it, and gate a regression. Eval calls are recorded in `eval_*` only, so a run
 * cannot move spend, routing or policy.
 */
export default async function EvalsPage(props: {
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({ locale, namespace: 'EvalsPage' });
  const data = await InferonApi.fromEnv().evals(session.cookie);
  const canEdit = data.role === 'owner';

  return (
    <>
      <PageHeader title={t('title_bar')} description={t('title_bar_description')} />
      <Card title={t('gold_sets')}>
        <EvalsPanel goldSets={data.gold_sets} runs={data.runs} canEdit={canEdit} />
      </Card>
    </>
  );
}

export const dynamic = 'force-dynamic';
