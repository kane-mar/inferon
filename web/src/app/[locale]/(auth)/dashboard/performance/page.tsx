// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { redirect } from 'next/navigation';

/** The performance report split into Runtime and Payloads (ITEM-103). */
export default function PerformanceRedirect() {
  redirect('/dashboard/runtime');
}
