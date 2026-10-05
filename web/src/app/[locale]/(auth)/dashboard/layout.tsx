import type { Metadata } from 'next';
import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { DashboardHeader } from '@/features/dashboard/DashboardHeader';
import { dashboardNav } from '@/libs/dashboardNav';
import { currentSession } from '@/libs/session';

type DashboardLayoutProps = {
  params: Promise<{ locale: string }>;
  children: React.ReactNode;
};

export async function generateMetadata(props: DashboardLayoutProps): Promise<Metadata> {
  const { locale } = await props.params;
  const t = await getTranslations({
    locale,
    namespace: 'DashboardLayout',
  });

  return {
    title: t('meta_title'),
    description: t('meta_description'),
  };
}

export default async function DashboardLayout(props: DashboardLayoutProps) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  const session = await currentSession();
  if (!session) {
    redirect('/auth/login');
  }

  const t = await getTranslations({
    locale,
    namespace: 'DashboardNav',
  });
  const labels = {
    home: t('home'),
    keys: t('keys'),
    members: t('members'),
    operator: t('operator'),
  };
  const menu = dashboardNav(session.me.is_operator).map(item => ({
    href: item.href,
    label: labels[item.labelKey],
  }));

  return (
    <>
      <div className="shadow-md">
        <div className="
          mx-auto flex max-w-7xl items-center justify-between px-3 py-4
        "
        >
          <DashboardHeader menu={menu} />
        </div>
      </div>

      <div className="min-h-[calc(100vh-72px)] bg-muted">
        <div className="mx-auto max-w-7xl px-3 pt-6 pb-16">
          {props.children}
        </div>
      </div>
    </>
  );
}

export const dynamic = 'force-dynamic';
