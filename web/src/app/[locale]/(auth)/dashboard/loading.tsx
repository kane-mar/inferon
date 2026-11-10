// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { SkeletonCards, SkeletonHeader, SkeletonRows } from '@/features/dashboard/Skeleton';

/**
 * ITEM-146: the shape of a report, shown while the report is being read.
 *
 * A `loading.tsx` here covers every route under the dashboard (each page streams
 * its own content into this shell), so a slow query looks like work in progress
 * rather than a click that did nothing.
 */
const Loading = () => (
  <div>
    <SkeletonHeader />
    <SkeletonCards />
    <div className="mt-6">
      <SkeletonRows />
    </div>
  </div>
);

export default Loading;
