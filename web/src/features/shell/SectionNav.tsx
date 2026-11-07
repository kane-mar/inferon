// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

'use client';

import type { DashboardNavEntry } from '@/libs/dashboardNav';
import { activeSection, isCurrent } from '@/libs/dashboardNav';
import { Link, usePathname } from '@/libs/I18nNavigation';
import { cn } from '@/utils/Helpers';

/**
 * The second-level menu for the section the page actually belongs to. Renders
 * nothing outside a section that has details, so most pages get no extra chrome
 * — and Providers never shows the dashboard's report tabs just because both
 * live under `/dashboard`.
 */
export const SectionNav = (props: { menu: DashboardNavEntry[] }) => {
  const pathname = usePathname();
  const section = activeSection(props.menu, pathname);

  if (!section?.children) {
    return null;
  }

  return (
    <div className="border-t bg-muted/40">
      <nav className="mx-auto max-w-7xl px-3">
        <ul className="flex flex-wrap gap-1 py-1.5 text-sm">
          {section.children.map(child => (
            <li key={child.href}>
              <Link
                href={child.href}
                className={cn(
                  'rounded-md px-2.5 py-1 text-muted-foreground',
                  'hover:bg-background hover:text-foreground',
                  isCurrent(pathname, child) && `
                    bg-background font-medium text-foreground shadow-sm
                  `,
                )}
              >
                {child.label}
              </Link>
            </li>
          ))}
        </ul>
      </nav>
    </div>
  );
};
