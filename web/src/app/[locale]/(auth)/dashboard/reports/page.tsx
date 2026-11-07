// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { redirect } from 'next/navigation';

/**
 * The Reports section is gone: its pages are tabs under the dashboard
 * (`dashboardNav`). The URL stays valid as a redirect so old links keep working.
 */
export default function ReportsRedirect() {
  redirect('/dashboard');
}
