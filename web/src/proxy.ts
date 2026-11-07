// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import createMiddleware from 'next-intl/middleware';
import { routing } from './libs/I18nRouting';

export default createMiddleware(routing);

export const config = {
  // Match all pathnames except for
  // - `/api/*` (route handlers, e.g. the health probe)
  // - … if they start with `/_next`, `/_vercel` or `monitoring`
  // - … the ones containing a dot (e.g. `favicon.ico`)
  matcher: '/((?!api|_next|_vercel|monitoring|.*\\..*).*)',
};
