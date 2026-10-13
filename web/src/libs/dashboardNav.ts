import type messages from '@/locales/en.json';

/** Keys under the `DashboardNav` message namespace. */
export type DashboardNavKey = keyof typeof messages.DashboardNav & string;

export type DashboardNavItem = { href: string; labelKey: DashboardNavKey };

/** The dashboard menu; the operator console is only for platform operators. */
export const dashboardNav = (isOperator: boolean): DashboardNavItem[] => [
  { href: '/dashboard', labelKey: 'home' },
  { href: '/dashboard/onboarding', labelKey: 'connect' },
  { href: '/dashboard/providers', labelKey: 'providers' },
  { href: '/dashboard/routing', labelKey: 'routing' },
  { href: '/dashboard/quality', labelKey: 'quality' },
  { href: '/dashboard/keys', labelKey: 'keys' },
  { href: '/dashboard/members', labelKey: 'members' },
  ...(isOperator
    ? [{ href: '/dashboard/operator', labelKey: 'operator' } satisfies DashboardNavItem]
    : []),
];
