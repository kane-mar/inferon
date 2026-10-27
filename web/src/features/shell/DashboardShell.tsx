import type { DashboardNavEntry } from '@/libs/dashboardNav';
import { useTranslations } from 'next-intl';
import { LocaleSwitcher } from '@/components/LocaleSwitcher';
import { Link } from '@/libs/I18nNavigation';
import { Logo } from './Logo';
import { MobileMenu } from './MobileMenu';
import { NavLink } from './NavLink';

/** The dashboard header: brand, section nav, locale + sign-out. */
export const DashboardShell = (props: { menu: DashboardNavEntry[] }) => {
  const t = useTranslations('DashboardHeader');

  return (
    <>
      <div className="flex items-center gap-3">
        <Link href="/dashboard" className="max-sm:hidden">
          <Logo />
        </Link>

        <nav className="max-lg:hidden">
          <ul className="flex flex-row items-center gap-x-1 text-sm font-medium">
            {props.menu.map(item => (
              <li key={item.href}>
                <NavLink href={item.href}>{item.label}</NavLink>
              </li>
            ))}
          </ul>
        </nav>
      </div>

      <div className="flex items-center gap-x-1.5">
        <div className="lg:hidden">
          <MobileMenu menu={props.menu} label={t('menu')} />
        </div>

        <LocaleSwitcher />

        <form action="/auth/logout" method="post">
          <button
            type="submit"
            className="
              rounded-md px-3 py-2 text-sm font-medium text-muted-foreground
              hover:text-foreground
            "
          >
            {t('sign_out')}
          </button>
        </form>
      </div>
    </>
  );
};
