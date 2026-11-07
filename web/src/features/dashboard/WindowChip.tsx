// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

'use client';

import { useTranslations } from 'next-intl';
import { usePathname, useRouter, useSearchParams } from 'next/navigation';
import { cn } from '@/utils/Helpers';
import { WINDOWS } from './reportWindow';

/**
 * The report window, stated and switchable in one place (ITEM-106).
 *
 * Every report reads `days` from the URL, so the chip is the only window UI and
 * no page can quietly hard-code a different period.
 */
export const WindowChip = (props: { days: number }) => {
  const t = useTranslations('WindowChip');
  const router = useRouter();
  const pathname = usePathname();
  const params = useSearchParams();

  const go = (days: number) => {
    const query = new URLSearchParams(params.toString());
    query.set('days', String(days));
    router.push(`${pathname}?${query.toString()}`);
  };

  return (
    <div className="flex items-center gap-2 text-sm">
      <span className="text-muted-foreground">{t('label')}</span>
      <div className="flex gap-1">
        {WINDOWS.map(window => (
          <button
            key={window}
            type="button"
            onClick={() => go(window)}
            className={cn(
              'rounded-md px-2 py-0.5 text-xs',
              window === props.days
                ? 'bg-foreground font-medium text-background'
                : `
                  text-muted-foreground
                  hover:text-foreground
                `,
            )}
          >
            {t('days', { days: window })}
          </button>
        ))}
      </div>
    </div>
  );
};
