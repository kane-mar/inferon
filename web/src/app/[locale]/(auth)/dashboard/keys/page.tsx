// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { redirect } from 'next/navigation';

/** Consolidated into Settings. The URL stays valid for old links. */
export default function SettingsRedirect() {
  redirect('/dashboard/settings');
}
