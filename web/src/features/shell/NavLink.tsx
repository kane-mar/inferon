'use client';

import { Link, usePathname } from '@/libs/I18nNavigation';
import { cn } from '@/utils/Helpers';

/** A localized nav link that highlights the section currently in view. */
export const NavLink = (props: { href: string; children: React.ReactNode }) => {
  const pathname = usePathname();
  // `/dashboard` must not light up on every child page, while a section like
  // `/dashboard/reports` stays active on `/dashboard/reports/spend`.
  const active = props.href === '/dashboard'
    ? /(?:^|\/)dashboard\/?$/.test(pathname)
    : pathname.endsWith(props.href) || pathname.includes(`${props.href}/`);

  return (
    <Link
      href={props.href}
      className={cn(
        'block rounded-md px-3 py-2',
        active && 'bg-primary text-primary-foreground',
      )}
    >
      {props.children}
    </Link>
  );
};
