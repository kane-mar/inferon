// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * ITEM-146: what a page looks like while its data is in flight.
 *
 * Every dashboard route used to block on its queries with nothing on screen, so a
 * slow report read as a stuck click. A skeleton keeps the shape of the page and
 * says "this is arriving", which is the difference between slow and broken. The
 * pulse is disabled under `prefers-reduced-motion` by the global rule.
 */
export const Skeleton = (props: { className?: string }) => (
  <div
    aria-hidden
    className={[
      'animate-pulse rounded-sm bg-muted',
      props.className ?? 'h-4 w-full',
    ].join(' ')}
  />
);

/** A page header's shape: title, then the controls on the right. */
export const SkeletonHeader = () => (
  <div className="mb-8 flex flex-wrap items-start justify-between gap-4">
    <Skeleton className="h-7 w-40" />
    <Skeleton className="h-8 w-28" />
  </div>
);

/** Three cards of nothing, the shape most reports open with. */
export const SkeletonCards = (props: { count?: number }) => (
  <div className="
    grid grid-cols-1 gap-4
    sm:grid-cols-2
    lg:grid-cols-3
  "
  >
    {Array.from({ length: props.count ?? 3 }, (_, index) => (
      <div key={index} className="rounded-lg border bg-card p-5">
        <Skeleton className="h-4 w-24" />
        <Skeleton className="mt-3 h-7 w-32" />
        <Skeleton className="mt-3 h-3 w-20" />
      </div>
    ))}
  </div>
);

/** A table's shape: a header rule and evenly spaced rows. */
export const SkeletonRows = (props: { rows?: number }) => (
  <div className="rounded-lg border bg-card p-5">
    <Skeleton className="h-4 w-32" />
    <div className="mt-4 space-y-3">
      {Array.from({ length: props.rows ?? 6 }, (_, index) => (
        <Skeleton key={index} className="h-3.5 w-full" />
      ))}
    </div>
  </div>
);
