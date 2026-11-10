// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from 'vitest';
import { activeSection, dashboardNav, matchDepth } from './dashboardNav';

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
      '/dashboard/payloads',
      '/dashboard/datasets',
    ]);
  });

  it('renders providers as a single page', () => {
    const providers = dashboardNav(false).find(item => item.href === '/dashboard/providers');

    expect(providers?.children).toBeUndefined();
  });

  it('keeps Settings a single page, with no submenu', () => {
    // Connect, keys and members are sections of the page now, not tabs.
    const settings = dashboardNav(false).find(item => item.href === '/dashboard/settings');

    expect(settings?.children).toBeUndefined();
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

describe('activeSection — a page belongs to its deepest menu entry', () => {
  const labels = (pathname: string, isOperator = true): string[] => {
    const menu = dashboardNav(isOperator).map(section => ({
      href: section.href,
      label: section.href,
      ...(section.children
        ? {
            children: section.children.map(child => ({
              href: child.href,
              label: child.href,
              ...(child.exact ? { exact: true } : {}),
            })),
          }
        : {}),
    }));
    return (activeSection(menu, pathname)?.children ?? []).map(child => child.href);
  };

  it('shows the report tabs on the dashboard pages', () => {
    expect(labels('/dashboard')).toContain('/dashboard/payloads');
    expect(labels('/dashboard/spend')).toContain('/dashboard/payloads');
  });

  it('keeps Runtime with the operator, not in the customer tabs', () => {
    // ITEM-140: the gateway's own microbenchmarks serve the honesty stance and the
    // operator; a customer's dashboard is about their bill.
    expect(labels('/dashboard')).not.toContain('/dashboard/runtime');
    expect(labels('/dashboard/runtime', true)).toEqual([]);

    const operator = dashboardNav(true).find(item => item.href === '/dashboard/operator');

    expect(hrefs(operator?.children ?? [])).toEqual(['/dashboard/runtime']);
  });

  it('shows no tabs on providers, routing or the operator console', () => {
    // These live under /dashboard too, which is exactly why the depth matters.
    expect(labels('/dashboard/providers')).toEqual([]);
    expect(labels('/dashboard/routing')).toEqual([]);
    expect(labels('/dashboard/operator')).toEqual([]);
  });

  it('shows no tabs on Settings — it is one page', () => {
    expect(labels('/dashboard/settings')).toEqual([]);
  });

  it('scopes a nested path to its own section, not the dashboard', () => {
    const menu = dashboardNav(false).map(section => ({
      href: section.href,
      label: section.href,
      ...(section.children
        ? { children: section.children.map(child => ({ href: child.href, label: child.href })) }
        : {}),
    }));
    const dashboard = menu.find(section => section.href === '/dashboard')!;
    const providers = menu.find(section => section.href === '/dashboard/providers')!;

    expect(matchDepth(dashboard, '/dashboard/providers')).toBe('/dashboard'.length);
    expect(matchDepth(providers, '/dashboard/providers')).toBe(
      '/dashboard/providers'.length,
    );
    expect(activeSection(menu, '/dashboard/providers')?.href).toBe('/dashboard/providers');
  });
});
