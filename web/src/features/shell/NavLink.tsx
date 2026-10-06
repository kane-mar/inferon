'use client';

import { Link, usePathname } from '@/libs/I18nNavigation';
import { cn } from '@/utils/Helpers';

/** A localized nav link that highlights the section currently in view. */
export const NavLink = (props: { href: string; children: React.ReactNode }) => {
  const pathname = usePathname();

  return (
    <Link
      href={props.href}
      className={cn(
        'block rounded-md px-3 py-2',
        pathname.endsWith(props.href) && 'bg-primary text-primary-foreground',
      )}
    >
      {props.children}
    </Link>
  );
};
