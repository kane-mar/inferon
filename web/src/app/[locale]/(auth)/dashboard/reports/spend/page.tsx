// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { redirect } from 'next/navigation';

/** The spend report moved to the dashboard's Spend tab. */
export default function SpendRedirect() {
  redirect('/dashboard/spend');
}
