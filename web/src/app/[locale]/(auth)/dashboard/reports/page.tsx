import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { PageHeader } from '@/features/shell/PageHeader';
import { Link } from '@/libs/I18nNavigation';
import { currentSession } from '@/libs/session';

/** The Reports hub: the detail behind the dashboard, in three places. */
export default async function ReportsPage(props: {
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({ locale, namespace: 'ReportsPage' });
  const cards = [
    {
      href: '/dashboard/reports/spend',
      title: t('spend_title'),
      body: t('spend_body'),
    },
    { href: '/dashboard/audit', title: t('audit_title'), body: t('audit_body') },
    {
      href: '/dashboard/quality',
      title: t('quality_title'),
      body: t('quality_body'),
    },
  ];

  return (
    <>
      <PageHeader title={t('title_bar')} description={t('title_bar_description')} />
      <div className="
        grid grid-cols-1 gap-4
        lg:grid-cols-3
      "
      >
        {cards.map(card => (
          <Link
            key={card.href}
            href={card.href}
            className="
              rounded-lg border bg-card p-4 shadow-sm
              hover:border-primary
            "
          >
            <h2 className="font-semibold">{card.title}</h2>
            <p className="mt-1 text-sm text-muted-foreground">{card.body}</p>
          </Link>
        ))}
      </div>
    </>
  );
}
