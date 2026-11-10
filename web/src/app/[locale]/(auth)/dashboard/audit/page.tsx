// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { redirect } from 'next/navigation';

/**
 * ITEM-142: the audit is the second half of the bill, not a page of its own.
 *
 * "What did we spend" and "where did it leak" are one question, and asking a
 * customer to know that is the kind of navigation this product should not have.
 * The findings now render on the bill page; this route stays so old links and
 * bookmarks land somewhere sensible.
 */
export default async function AuditRedirect(props: {
  params: Promise<{ locale: string }>;
  searchParams: Promise<{ days?: string }>;
}) {
  const { locale } = await props.params;
  const params = await props.searchParams;
  const query = params.days ? `?days=${params.days}` : '';
  redirect(`/${locale}/dashboard/spend${query}`);
}
