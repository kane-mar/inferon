import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { ControlsPanel } from '@/features/dashboard/ControlsPanel';
import { PrunePanel } from '@/features/dashboard/PrunePanel';
import { HEAD } from '@/features/dashboard/reportFormat';
import { RoutingPanel } from '@/features/dashboard/RoutingPanel';
import { TierPairsPanel } from '@/features/dashboard/TierPairsPanel';
import { PageHeader } from '@/features/shell/PageHeader';
import { InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

type RoutingPageProps = {
  params: Promise<{ locale: string }>;
};

/** A change value as the table shows it: never `[object Object]`. */
const auditValue = (value: unknown, yes: string, no: string): string => {
  if (value == null) {
    return '—';
  }
  if (typeof value === 'boolean') {
    return value ? yes : no;
  }
  if (typeof value === 'string') {
    return value;
  }
  return JSON.stringify(value);
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
  const modelIds = models.providers
    .flatMap(provider => provider.models)
    .filter(model => model.enabled)
    .map(model => model.id);

  return (
    <>
      <PageHeader title={t('title_bar')} description={t('title_bar_description')} />
      <Card title={t('chain')} icon="layers">
        <RoutingPanel data={data} modelIds={modelIds} />
      </Card>

      <div className="mt-4">
        <Card title={t('tier_pairs')} icon="prune">
          <TierPairsPanel
            pairs={data.tier_pairs}
            modelIds={modelIds}
            canEdit={data.role === 'owner'}
          />
        </Card>
      </div>

      <div className="mt-4">
        <Card title={t('prune')} icon="prune">
          <PrunePanel policy={config.prune} canEdit={config.role === 'owner'} />
        </Card>
      </div>

      <div className="mt-4">
        <Card title={t('controls')} icon="activity">
          <ControlsPanel data={config} modelIds={modelIds} />
        </Card>
      </div>

      {data.role === 'owner' && (
        <div className="mt-4">
          <Card title={t('audit')} icon="lock">
            <p className="mb-3 text-sm text-muted-foreground">{t('audit_help')}</p>
            {data.audit.length === 0
              ? <p className="text-sm text-muted-foreground">{t('audit_empty')}</p>
              : (
                  <div className="overflow-x-auto">
                    <table className="w-full text-left text-sm">
                      <thead>
                        <tr className="border-b">
                          <th className={HEAD}>{t('audit_when')}</th>
                          <th className={HEAD}>{t('audit_actor')}</th>
                          <th className={HEAD}>{t('audit_setting')}</th>
                          <th className={HEAD}>{t('audit_change')}</th>
                        </tr>
                      </thead>
                      <tbody>
                        {data.audit.map((entry) => {
                          const before = auditValue(entry.before, t('on'), t('off'));
                          const after = auditValue(entry.after, t('on'), t('off'));
                          return (
                            <tr
                              key={`${entry.at}-${entry.setting}-${entry.actor_login}-${JSON.stringify(entry.after)}`}
                              className="
                                border-b
                                last:border-0
                              "
                            >
                              <td className="
                                py-2 font-mono text-xs text-muted-foreground
                              "
                              >
                                <span title={entry.at}>
                                  {entry.at.slice(0, 16).replace('T', ' ')}
                                </span>
                              </td>
                              <td className="py-2 font-mono text-xs">{entry.actor_login}</td>
                              <td className="py-2">{t(`setting_${entry.setting}`)}</td>
                              <td className="py-2">
                                <span
                                  className="
                                    inline-block max-w-96 truncate align-bottom
                                    font-mono text-xs text-muted-foreground
                                  "
                                  title={before}
                                >
                                  {before}
                                </span>
                                <span className="px-1 text-muted-foreground">→</span>
                                <span
                                  className="
                                    inline-block max-w-96 truncate align-bottom
                                    font-mono text-xs
                                  "
                                  title={after}
                                >
                                  {after}
                                </span>
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                )}
          </Card>
        </div>
      )}
    </>
  );
}
