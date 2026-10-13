import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { Card } from '@/features/dashboard/Card';
import { KpiCard } from '@/features/dashboard/KpiCard';
import { PageHeader } from '@/features/shell/PageHeader';
import { InferonApi } from '@/libs/InferonApi';
import { currentSession } from '@/libs/session';

const HEAD = 'px-3 py-2 text-left font-mono text-xs uppercase text-muted-foreground';
const CELL = 'px-3 py-2 text-sm tabular-nums';

const pct = (value: number | null): string =>
  (value == null ? '—' : `${(value * 100).toFixed(1)}%`);

/**
 * Quality view (ITEM-055): how often responses were usable, derived from the
 * response envelope, plus optional client thumbs and the guard-fallback rate.
 */
export default async function QualityPage(props: {
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({ locale, namespace: 'QualityPage' });
  const data = await InferonApi.fromEnv().quality(30, session.cookie);
  const overall = data.overall;

  return (
    <>
      <PageHeader title={t('title_bar')} description={t('title_bar_description')} />

      <div className="
        mt-6 grid grid-cols-2 gap-3
        lg:grid-cols-3
        xl:grid-cols-5
      "
      >
        <KpiCard label={t('success_rate')} value={pct(overall.success_rate)} />
        <KpiCard label={t('empty_rate')} value={pct(overall.empty_rate)} goodWhen="down" />
        <KpiCard label={t('refusal_rate')} value={pct(overall.refusal_rate)} goodWhen="down" />
        <KpiCard label={t('error_rate')} value={pct(overall.error_rate)} goodWhen="down" />
        <KpiCard
          label={t('guard_fallback_rate')}
          value={pct(overall.guard_fallback_rate)}
          goodWhen="down"
        />
      </div>

      <div className="mt-4">
        <Card title={t('by_model')}>
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead>
                <tr className="border-b">
                  <th className={HEAD}>{t('model')}</th>
                  <th className={HEAD}>{t('requests')}</th>
                  <th className={HEAD}>{t('ok')}</th>
                  <th className={HEAD}>{t('empty')}</th>
                  <th className={HEAD}>{t('errors')}</th>
                  <th className={HEAD}>{t('success')}</th>
                  <th className={HEAD}>{t('thumbs_up')}</th>
                  <th className={HEAD}>{t('thumbs_down')}</th>
                </tr>
              </thead>
              <tbody>
                {data.models.map(model => (
                  <tr
                    key={model.model}
                    className="
                      border-b
                      last:border-0
                    "
                  >
                    <td className={`
                      ${CELL}
                      font-mono
                    `}
                    >
                      {model.model}
                    </td>
                    <td className={CELL}>{model.requests}</td>
                    <td className={CELL}>{model.ok}</td>
                    <td className={CELL}>{model.empty}</td>
                    <td className={CELL}>{model.errors}</td>
                    <td className={CELL}>{pct(model.success_rate)}</td>
                    <td className={CELL}>{model.thumbs_up}</td>
                    <td className={CELL}>{model.thumbs_down}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      </div>

      <div className="mt-4">
        <Card title={t('by_prompt')}>
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead>
                <tr className="border-b">
                  <th className={HEAD}>{t('cluster')}</th>
                  <th className={HEAD}>{t('requests')}</th>
                  <th className={HEAD}>{t('ok')}</th>
                  <th className={HEAD}>{t('empty')}</th>
                  <th className={HEAD}>{t('errors')}</th>
                  <th className={HEAD}>{t('success')}</th>
                </tr>
              </thead>
              <tbody>
                {data.prompts.map(prompt => (
                  <tr
                    key={prompt.system_prompt_hash}
                    className="
                      border-b
                      last:border-0
                    "
                  >
                    <td className={`
                      ${CELL}
                      font-mono
                    `}
                    >
                      {prompt.system_prompt_hash.slice(0, 12)}
                    </td>
                    <td className={CELL}>{prompt.requests}</td>
                    <td className={CELL}>{prompt.ok}</td>
                    <td className={CELL}>{prompt.empty}</td>
                    <td className={CELL}>{prompt.errors}</td>
                    <td className={CELL}>{pct(prompt.success_rate)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      </div>
    </>
  );
}
