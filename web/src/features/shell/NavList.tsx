// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

'use client';

import type { DashboardNavEntry } from '@/libs/dashboardNav';
import { usePathname } from '@/libs/I18nNavigation';
import { NavLink } from './NavLink';

/** A section is active on its own page and on any of its details. */
const activeFor = (pathname: string, entry: DashboardNavEntry): boolean =>
  [entry.href, ...(entry.children?.map(child => child.href) ?? [])].some(href =>
    href === '/dashboard'
      ? /(?:^|\/)dashboard\/?$/.test(pathname)
      : pathname.endsWith(href) || pathname.includes(`${href}/`),
  );

/** The top-level section links (the details live in `SectionNav`). */
export const NavList = (props: { menu: DashboardNavEntry[] }) => {
  const pathname = usePathname();

  return (
    <ul className="flex flex-row items-center gap-x-1 text-sm font-medium">
      {props.menu.map(item => (
        <li key={item.href}>
          <NavLink href={item.href} active={activeFor(pathname, item)}>
            {item.label}
          </NavLink>
        </li>
      ))}
    </ul>
  );
};
