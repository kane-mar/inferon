// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

'use client';

import { Link } from '@/libs/I18nNavigation';
import { cn } from '@/utils/Helpers';

/** A localized nav link, highlighted when its section is the current one. */
export const NavLink = (props: {
  href: string;
  active: boolean;
  children: React.ReactNode;
}) => (
  <Link
    href={props.href}
    className={cn(
      'block rounded-md px-3 py-2',
      props.active && 'bg-primary text-primary-foreground',
    )}
  >
    {props.children}
  </Link>
);
