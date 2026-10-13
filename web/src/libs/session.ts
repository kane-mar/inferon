import type { Me } from './InferonApi';
import { cookies } from 'next/headers';
import { cache } from 'react';
import { InferonApi, InferonApiError } from './InferonApi';

const SESSION_COOKIE = 'inferon_session';

/** Returns a `Cookie` request-header value for the session, if present. */
const sessionCookie = async (): Promise<string | undefined> => {
  const token = (await cookies()).get(SESSION_COOKIE)?.value;
  return token ? `${SESSION_COOKIE}=${token}` : undefined;
};

/** Resolves the signed-in user; `null` for a missing or expired session. */
export const resolveMe = async (
  api: InferonApi,
  cookie: string | undefined,
): Promise<Me | null> => {
  if (!cookie) {
    return null;
  }
  try {
    return await api.me(cookie);
  } catch (error) {
    if (error instanceof InferonApiError && error.status === 401) {
      return null;
    }
    throw error;
  }
};

export type Session = { cookie: string; me: Me };

/** The request's session, de-duplicated across the dashboard layout and pages. */
export const currentSession = cache(async (): Promise<Session | null> => {
  const cookie = await sessionCookie();
  if (!cookie) {
    return null;
  }
  const me = await resolveMe(InferonApi.fromEnv(), cookie);
  return me ? { cookie, me } : null;
});
