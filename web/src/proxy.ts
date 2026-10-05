import createMiddleware from 'next-intl/middleware';
import { routing } from './libs/I18nRouting';

export default createMiddleware(routing);

export const config = {
  // Match all pathnames except for
  // - … if they start with `/_next`, `/_vercel` or `monitoring`
  // - … the ones containing a dot (e.g. `favicon.ico`)
  matcher: '/((?!_next|_vercel|monitoring|.*\\..*).*)',
};
