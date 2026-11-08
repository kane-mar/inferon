// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

'use client';

import { Link } from '@/libs/I18nNavigation';
import { cn } from '@/utils/Helpers';

/**
 * A localized nav link. The current section takes a 15% accent wash and the
 * accent ink — the one place on the page where colour marks *where you are*, so
 * it is the only nav state that needs any.
 */
export const NavLink = (props: {
  href: string;
  active: boolean;
  children: React.ReactNode;
}) => (
  <Link
    href={props.href}
    className={cn(
      'block rounded-md px-3 py-2 transition-colors',
      props.active
        ? 'bg-accent/15 font-medium text-accent-ink'
        : `
          text-muted-foreground
          hover:bg-accent/8 hover:text-foreground
        `,
    )}
  >
    {props.children}
  </Link>
);
