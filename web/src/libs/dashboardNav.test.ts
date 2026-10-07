import { describe, expect, it } from 'vitest';
import { dashboardNav } from './dashboardNav';

describe('dashboardNav — role-aware menu', () => {
  it('gives tenants home, connect, providers, routing, keys and members', () => {
    expect(dashboardNav(false).map(item => item.href)).toEqual([
      '/dashboard',
      '/dashboard/onboarding',
      '/dashboard/providers',
      '/dashboard/routing',
      '/dashboard/keys',
      '/dashboard/members',
    ]);
  });

  it('adds the operator console last for operators', () => {
    const nav = dashboardNav(true);

    expect(nav.map(item => item.href)).toEqual([
      '/dashboard',
      '/dashboard/onboarding',
      '/dashboard/providers',
      '/dashboard/routing',
      '/dashboard/keys',
      '/dashboard/members',
      '/dashboard/operator',
    ]);
    expect(nav.at(-1)?.labelKey).toBe('operator');
  });
});
