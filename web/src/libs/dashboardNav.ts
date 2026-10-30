import type messages from '@/locales/en.json';

/** Keys under the `DashboardNav` message namespace. */
export type DashboardNavKey = keyof typeof messages.DashboardNav & string;

export type DashboardNavItem = { href: string; labelKey: DashboardNavKey };

/** A top-level section; `children` render as a second-level submenu. */
export type DashboardNavSection = DashboardNavItem & { children?: DashboardNavItem[] };

/** A section with its labels resolved, as the shell receives it. */
export type DashboardNavEntry = {
  href: string;
  label: string;
  children?: { href: string; label: string }[];
};

/**
 * The dashboard menu: five sections (plus the operator console), each with its
 * details in a submenu rather than as top-level tabs.
 */
export const dashboardNav = (isOperator: boolean): DashboardNavSection[] => [
  { href: '/dashboard', labelKey: 'home' },
  { href: '/dashboard/providers', labelKey: 'providers' },
  { href: '/dashboard/routing', labelKey: 'routing' },
  {
    href: '/dashboard/reports',
    labelKey: 'reports',
    children: [
      { href: '/dashboard/reports/spend', labelKey: 'spend' },
      { href: '/dashboard/audit', labelKey: 'audit' },
      { href: '/dashboard/quality', labelKey: 'quality' },
      { href: '/dashboard/performance', labelKey: 'performance' },
      { href: '/dashboard/datasets', labelKey: 'datasets' },
    ],
  },
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
