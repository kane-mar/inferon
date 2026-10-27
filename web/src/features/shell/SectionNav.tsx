'use client';

import type { DashboardNavEntry } from '@/libs/dashboardNav';
import { Link, usePathname } from '@/libs/I18nNavigation';
import { cn } from '@/utils/Helpers';

/** True when `href` is the current page or an ancestor of it. */
const within = (pathname: string, href: string): boolean =>
  pathname.endsWith(href) || pathname.includes(`${href}/`);

/**
 * The second-level menu for the active section. Renders nothing outside a
 * section that has details, so most pages get no extra chrome.
 */
export const SectionNav = (props: { menu: DashboardNavEntry[] }) => {
  const pathname = usePathname();
  const section = props.menu.find(
    item =>
      item.children
      && (within(pathname, item.href)
        || item.children.some(child => within(pathname, child.href))),
  );

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
                  within(pathname, child.href) && `
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
