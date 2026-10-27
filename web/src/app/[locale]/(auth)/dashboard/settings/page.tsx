import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { PageHeader } from '@/features/shell/PageHeader';
import { Link } from '@/libs/I18nNavigation';
import { currentSession } from '@/libs/session';

/** The Settings hub: onboarding, access, and the on-prem node. */
export default async function SettingsPage(props: {
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({ locale, namespace: 'SettingsPage' });
  const cards = [
    {
      href: '/dashboard/onboarding',
      title: t('connect_title'),
      body: t('connect_body'),
    },
    { href: '/dashboard/keys', title: t('keys_title'), body: t('keys_body') },
    {
      href: '/dashboard/members',
      title: t('members_title'),
      body: t('members_body'),
    },
  ];

  return (
    <>
      <PageHeader title={t('title_bar')} description={t('title_bar_description')} />
      <div className="
        grid grid-cols-1 gap-4
        lg:grid-cols-2
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
