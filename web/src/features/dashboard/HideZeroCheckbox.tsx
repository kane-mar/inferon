// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

'use client';

import { useSearchParams } from 'next/navigation';
import { usePathname, useRouter } from '@/libs/I18nNavigation';

/**
 * A checkbox that round-trips a boolean through the URL (`hide_zero=1`), so the
 * server component can filter rows without shipping any data to the client.
 */
export const HideZeroCheckbox = (props: { checked: boolean; label: string }) => {
  const router = useRouter();
  const pathname = usePathname();
  const params = useSearchParams();

  return (
    <label className="
      inline-flex cursor-pointer items-center gap-1.5 text-sm
      text-muted-foreground
      hover:text-foreground
    "
    >
      <input
        type="checkbox"
        className="size-3.5 accent-primary"
        checked={props.checked}
        onChange={(event) => {
          const next = new URLSearchParams(params.toString());
          if (event.target.checked) {
            next.set('hide_zero', '1');
          } else {
            next.delete('hide_zero');
          }
          const query = next.toString();
          router.replace(query ? `${pathname}?${query}` : pathname);
        }}
      />
      {props.label}
    </label>
  );
};
