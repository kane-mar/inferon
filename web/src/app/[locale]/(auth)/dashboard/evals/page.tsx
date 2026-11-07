// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { redirect } from 'next/navigation';

/** Folded into another report (ITEM-105); the URL stays valid. */
export default function EvalsRedirect() {
  redirect('/dashboard/quality');
}
