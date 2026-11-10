// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it, vi } from 'vitest';
import { InferonApi, InferonApiError } from './InferonApi';
import { resolveMe } from './session';

// `session.ts` imports the Next server runtime for its cookie, and `next/headers`
// cannot be required outside the bundler (its shim points at a file only the build
// emits). Mocking it is what makes this a unit test of the session seam rather
// than a test of Next's packaging.
vi.mock('next/headers', () => ({
  cookies: async () => ({ get: () => undefined }),
}));

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });

const user = { id: 'u1', github_id: 42, github_login: 'kane-mar', is_operator: true };

describe('resolveMe — the dashboard session seam', () => {
  it('returns null without a session cookie and never calls the API', async () => {
    const fetchImpl = vi.fn();

    await expect(resolveMe(new InferonApi({ fetchImpl }), undefined)).resolves.toBeNull();
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it('returns the signed-in user for a valid cookie', async () => {
    const fetchImpl = vi.fn(async () => json(user));

    const me = await resolveMe(
      new InferonApi({ baseUrl: 'http://proxy:8080', fetchImpl }),
      'inferon_session=t',
    );

    expect(me).toEqual(user);
  });

  it('treats 401 as signed out but propagates other failures', async () => {
    const unauthorized = vi.fn(async () =>
      json({ error: { message: 'not signed in', type: 'unauthorized' } }, 401),
    );

    await expect(
      resolveMe(new InferonApi({ fetchImpl: unauthorized }), 'inferon_session=t'),
    ).resolves.toBeNull();

    const broken = vi.fn(async () =>
      json({ error: { message: 'boom', type: 'internal' } }, 500),
    );

    await expect(
      resolveMe(new InferonApi({ fetchImpl: broken }), 'inferon_session=t'),
    ).rejects.toBeInstanceOf(InferonApiError);
  });
});
