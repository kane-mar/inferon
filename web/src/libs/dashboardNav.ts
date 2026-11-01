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
      { href: '/dashboard/performance', labelKey: 'performance' },
      { href: '/dashboard/datasets', labelKey: 'datasets' },
    ],
  },
  { href: '/dashboard/providers', labelKey: 'providers' },
  { href: '/dashboard/routing', labelKey: 'routing' },
  {
    href: '/dashboard/settings',
    labelKey: 'settings',
    children: [
      { href: '/dashboard/onboarding', labelKey: 'connect' },
      { href: '/dashboard/keys', labelKey: 'keys' },
      { href: '/dashboard/members', labelKey: 'members' },
    ],
  },
  ...(isOperator
    ? [{ href: '/dashboard/operator', labelKey: 'operator' } satisfies DashboardNavSection]
    : []),
];
