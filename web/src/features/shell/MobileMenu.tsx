'use client';

import { Button } from '@/components/ui/button';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Link } from '@/libs/I18nNavigation';

/** The small-screen menu shared by the landing and dashboard headers. */
export const MobileMenu = (props: {
  menu: { href: string; label: string }[];
  label: string;
}) => (
  <DropdownMenu>
    <DropdownMenuTrigger asChild>
      <Button variant="ghost" size="icon" aria-label={props.label}>
        <svg
          className="size-6 stroke-current"
          xmlns="http://www.w3.org/2000/svg"
          viewBox="0 0 24 24"
          strokeWidth="1.5"
          fill="none"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M0 0h24v24H0z" stroke="none" />
          <path d="M4 6h16M4 12h16M4 18h16" />
        </svg>
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent>
      {props.menu.map(item => (
        <DropdownMenuItem key={item.href} asChild>
          {item.href.startsWith('http')
            ? <a href={item.href}>{item.label}</a>
            : <Link href={item.href}>{item.label}</Link>}
        </DropdownMenuItem>
      ))}
    </DropdownMenuContent>
  </DropdownMenu>
);
