// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import type messages from '@/locales/en.json';

/** Keys under the `DashboardNav` message namespace. */
export type DashboardNavKey = keyof typeof messages.DashboardNav & string;

export type DashboardNavItem = {
  href: string;
  labelKey: DashboardNavKey;
  /** Only this exact page is "current" (the section's own overview tab). */
  exact?: boolean;
};

/** A top-level section; `children` render as a second-level submenu. */
export type DashboardNavSection = DashboardNavItem & { children?: DashboardNavItem[] };

/** A section with its labels resolved, as the shell receives it. */
export type DashboardNavEntry = {
  href: string;
  label: string;
  children?: { href: string; label: string; exact?: boolean }[];
};

/**
 * The dashboard menu: four sections (plus the operator console).
 *
 * The reports are **tabs under Dashboard**, not their own section — the
 * dashboard is the one surface, and its tabs are the detail views.
 */
export const dashboardNav = (isOperator: boolean): DashboardNavSection[] => [
  {
    href: '/dashboard',
    labelKey: 'home',
    children: [
      { href: '/dashboard', labelKey: 'overview', exact: true },
      { href: '/dashboard/spend', labelKey: 'spend' },
      { href: '/dashboard/audit', labelKey: 'audit' },
      { href: '/dashboard/quality', labelKey: 'quality' },
      { href: '/dashboard/runtime', labelKey: 'runtime' },
      { href: '/dashboard/payloads', labelKey: 'payloads' },
      { href: '/dashboard/datasets', labelKey: 'datasets' },
    ],
  },
  { href: '/dashboard/providers', labelKey: 'providers' },
  { href: '/dashboard/routing', labelKey: 'routing' },
  // Settings is one page: connect, keys, members and the workspace summary.
  { href: '/dashboard/settings', labelKey: 'settings' },
  ...(isOperator
    ? [{ href: '/dashboard/operator', labelKey: 'operator' } satisfies DashboardNavSection]
    : []),
];

/** True when `href` is the page or an ancestor of it. */
export const within = (pathname: string, href: string): boolean =>
  pathname.endsWith(href) || pathname.includes(`${href}/`);

/**
 * The deepest part of the menu this section matches: its own href, or one of
 * its children's. Depth is the matched href's length, which is what makes the
 * choice correct — every dashboard page is "within" `/dashboard`, so a
 * shallow match must never win over `/dashboard/providers`.
 */
export const matchDepth = (section: DashboardNavEntry, pathname: string): number => {
  const hrefs = [section.href, ...(section.children ?? []).map(child => child.href)];

  return hrefs
    .filter(href => within(pathname, href))
    .reduce((deepest, href) => Math.max(deepest, href.length), 0);
};

/**
 * The section the page belongs to. Providers must not show the report tabs just
 * because both live under `/dashboard`; the deepest match decides.
 */
export const activeSection = (
  menu: DashboardNavEntry[],
  pathname: string,
): DashboardNavEntry | undefined => {
  let best: DashboardNavEntry | undefined;
  let bestDepth = 0;
  for (const section of menu) {
    const depth = matchDepth(section, pathname);
    if (depth > bestDepth) {
      best = section;
      bestDepth = depth;
    }
  }
  return best;
};

/**
 * The tab to highlight. The section's own overview tab points at the section
 * root, which every tab is "within", so it is marked `exact`.
 */
export const isCurrent = (
  pathname: string,
  item: { href: string; exact?: boolean },
): boolean => (item.exact ? pathname.endsWith(item.href) : within(pathname, item.href));
