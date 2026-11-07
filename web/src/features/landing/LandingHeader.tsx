// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { useTranslations } from 'next-intl';
import { AuthLink } from '@/components/AuthLink';
import { LocaleSwitcher } from '@/components/LocaleSwitcher';
import { buttonVariants } from '@/components/ui/buttonVariants';
import { Logo } from '@/features/shell/Logo';
import { MobileMenu } from '@/features/shell/MobileMenu';
import { Link } from '@/libs/I18nNavigation';

const REPOSITORY = 'https://github.com/kane-mar/inferon';

/** The marketing header — the same shell vocabulary as the dashboard. */
export const LandingHeader = () => {
  const t = useTranslations('Navbar');

  const menu = [
    { href: '/#features', label: t('features') },
    { href: REPOSITORY, label: t('source') },
  ];

  return (
    <header className="border-b">
      <div className="
        mx-auto flex max-w-7xl flex-wrap items-center justify-between px-3 py-4
      "
      >
        <Link href="/">
          <Logo />
        </Link>

        <nav className="max-lg:hidden">
          <ul className="
            flex gap-x-6 text-sm font-medium
            [&_a:hover]:opacity-70
          "
          >
            {menu.map(item => (
              <li key={item.href}>
                {item.href.startsWith('http')
                  ? <a href={item.href}>{item.label}</a>
                  : <Link href={item.href}>{item.label}</Link>}
              </li>
            ))}
          </ul>
        </nav>

        <div className="flex items-center gap-x-1.5">
          <div className="lg:hidden">
            <MobileMenu menu={menu} label={t('menu')} />
          </div>

          <LocaleSwitcher />

          <AuthLink className="mr-2.5 ml-1">{t('sign_in')}</AuthLink>

          <Link className={buttonVariants()} href="/dashboard">
            {t('dashboard')}
          </Link>
        </div>
      </div>
    </header>
  );
};
