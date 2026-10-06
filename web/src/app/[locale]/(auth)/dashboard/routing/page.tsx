import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { ControlsPanel } from '@/features/dashboard/ControlsPanel';
import { RoutingPanel } from '@/features/dashboard/RoutingPanel';
import { TitleBar } from '@/features/dashboard/TitleBar';
import { InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

type RoutingPageProps = {
  params: Promise<{ locale: string }>;
};

/**
 * Fallback & routing rules (ITEM-045): an ordered chain of models tried when the
 * primary request fails retryably (transport error, 429, 5xx). An empty chain
 * changes nothing.
 */
export default async function RoutingPage(props: RoutingPageProps) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({ locale, namespace: 'RoutingPage' });
  const api = InferonApi.fromEnv();
  const [data, models, config] = await Promise.all([
    api.routing(session.cookie),
    api.models(session.cookie),
    api.config(session.cookie),
  ]);

  return (
    <>
      <TitleBar title={t('title_bar')} description={t('title_bar_description')} />
      <Card title={t('chain')}>
        <RoutingPanel data={data} models={models.models} />
      </Card>

      <div className="mt-4">
        <Card title={t('controls')}>
          <ControlsPanel data={config} models={models.models} />
        </Card>
      </div>
    </>
  );
}
