// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import type { Metadata } from 'next';
import { getTranslations, setRequestLocale } from 'next-intl/server';
import { redirect } from 'next/navigation';
import { DashboardShell } from '@/features/shell/DashboardShell';
import { SectionNav } from '@/features/shell/SectionNav';
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
    overview: t('overview'),
    providers: t('providers'),
    routing: t('routing'),
    settings: t('settings'),
    bill: t('bill'),
    quality: t('quality'),
    runtime: t('runtime'),
    payloads: t('payloads'),
    datasets: t('datasets'),
    connect: t('connect'),
    keys: t('keys'),
    members: t('members'),
    operator: t('operator'),
  };
  const menu = dashboardNav(session.me.is_operator).map(item => ({
    href: item.href,
    label: labels[item.labelKey],
    ...(item.children
      ? {
          children: item.children.map(child => ({
            href: child.href,
            label: labels[child.labelKey],
            ...(child.exact ? { exact: true } : {}),
          })),
        }
      : {}),
  }));

  return (
    <>
      <header className="sticky top-0 z-20 border-b bg-background">
        <div className="mx-auto flex max-w-7xl items-center justify-between p-3">
          <DashboardShell menu={menu} />
        </div>
        <SectionNav menu={menu} />
      </header>

      {/* ITEM-143: the jump target for the skip link. */}
      <main id="main" className="min-h-[calc(100vh-57px)] bg-muted">
        <div className="mx-auto max-w-7xl px-3 pt-6 pb-16">
          {props.children}
        </div>
      </main>
    </>
  );
}

export const dynamic = 'force-dynamic';
