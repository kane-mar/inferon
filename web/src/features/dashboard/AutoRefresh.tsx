'use client';

import { useEffect } from 'react';
import { useRouter } from '@/libs/I18nNavigation';

/**
 * Re-render the current route's server components on a cadence, so a report
 * built from live telemetry stays live without shipping the data client-side.
 */
export const AutoRefresh = (props: { label: string; intervalMs?: number }) => {
  const router = useRouter();
  const intervalMs = props.intervalMs ?? 15_000;

  useEffect(() => {
    const timer = setInterval(() => router.refresh(), intervalMs);
    return () => clearInterval(timer);
  }, [router, intervalMs]);

  return (
    <span className="
      inline-flex items-center gap-1.5 text-xs text-muted-foreground
    "
    >
      <span className="size-1.5 animate-pulse rounded-full bg-emerald-500" aria-hidden />
      {props.label}
    </span>
  );
};
