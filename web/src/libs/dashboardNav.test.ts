import { describe, expect, it } from 'vitest';
import { dashboardNav } from './dashboardNav';

const hrefs = (items: { href: string }[]): string[] => items.map(item => item.href);

describe('dashboardNav — five sections, details in submenus', () => {
  it('keeps the top level to five sections for tenants', () => {
    expect(hrefs(dashboardNav(false))).toEqual([
      '/dashboard',
      '/dashboard/providers',
      '/dashboard/routing',
      '/dashboard/reports',
      '/dashboard/settings',
    ]);
  });

  it('puts the spend, leak and quality reports under Reports', () => {
    const reports = dashboardNav(false).find(item => item.href === '/dashboard/reports');

    expect(hrefs(reports?.children ?? [])).toEqual([
      '/dashboard/reports/spend',
      '/dashboard/leaks',
      '/dashboard/quality',
    ]);
  });

  it('puts access and setup under Settings', () => {
    const settings = dashboardNav(false).find(item => item.href === '/dashboard/settings');

    expect(hrefs(settings?.children ?? [])).toEqual([
      '/dashboard/onboarding',
      '/dashboard/keys',
      '/dashboard/members',
      '/dashboard/edge',
    ]);
  });

  it('adds the operator console last for operators', () => {
    const nav = dashboardNav(true);

    expect(hrefs(nav)).toEqual([
      '/dashboard',
      '/dashboard/providers',
      '/dashboard/routing',
      '/dashboard/reports',
      '/dashboard/settings',
      '/dashboard/operator',
    ]);
    expect(nav.at(-1)?.labelKey).toBe('operator');
  });
});
