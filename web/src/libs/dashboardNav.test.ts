import { describe, expect, it } from 'vitest';
import { dashboardNav } from './dashboardNav';

const hrefs = (items: { href: string }[]): string[] => items.map(item => item.href);

describe('dashboardNav — four sections, reports as dashboard tabs', () => {
  it('keeps the top level to four sections for tenants', () => {
    expect(hrefs(dashboardNav(false))).toEqual([
      '/dashboard',
      '/dashboard/providers',
      '/dashboard/routing',
      '/dashboard/settings',
    ]);
  });

  it('has no separate Reports section — the reports are dashboard tabs', () => {
    expect(dashboardNav(false).some(item => item.href === '/dashboard/reports')).toBe(false);
  });

  it('puts the report tabs under Dashboard, overview first', () => {
    const dashboard = dashboardNav(false).find(item => item.href === '/dashboard');

    expect(hrefs(dashboard?.children ?? [])).toEqual([
      '/dashboard',
      '/dashboard/spend',
      '/dashboard/audit',
      '/dashboard/quality',
      '/dashboard/performance',
      '/dashboard/datasets',
    ]);
  });

  it('renders providers as a single page (edge nodes live inside it)', () => {
    const providers = dashboardNav(false).find(item => item.href === '/dashboard/providers');

    expect(providers?.children).toBeUndefined();
  });

  it('puts access and setup under Settings', () => {
    const settings = dashboardNav(false).find(item => item.href === '/dashboard/settings');

    expect(hrefs(settings?.children ?? [])).toEqual([
      '/dashboard/onboarding',
      '/dashboard/keys',
      '/dashboard/members',
    ]);
  });

  it('adds the operator console last for operators', () => {
    const nav = dashboardNav(true);

    expect(hrefs(nav)).toEqual([
      '/dashboard',
      '/dashboard/providers',
      '/dashboard/routing',
      '/dashboard/settings',
      '/dashboard/operator',
    ]);
    expect(nav.at(-1)?.labelKey).toBe('operator');
  });
});
