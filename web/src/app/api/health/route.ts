// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/** Liveness probe for the web container (bypasses i18n middleware). */
export const dynamic = 'force-dynamic';

export function GET() {
  return Response.json({ status: 'ok' });
}
