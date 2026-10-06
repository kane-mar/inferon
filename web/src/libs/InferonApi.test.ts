import type { FetchLike } from './InferonApi';
import { describe, expect, it, vi } from 'vitest';
import { formatUsd, InferonApi, InferonApiError } from './InferonApi';

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });

const makeApi = (fetchImpl: FetchLike) =>
  new InferonApi({ baseUrl: 'http://proxy:8080', fetchImpl });

describe('InferonApi — session forwarding', () => {
  it('forwards the browser session cookie on server-side calls', async () => {
    const fetchImpl = vi.fn(async () =>
      json({ id: 'u1', github_id: 42, github_login: 'kane-mar', is_operator: true }),
    );
    const me = await makeApi(fetchImpl).me('inferon_session=abc');

    expect(me).toEqual({
      id: 'u1',
      github_id: 42,
      github_login: 'kane-mar',
      is_operator: true,
    });

    const [url, init] = fetchImpl.mock.calls[0] as unknown as [string, RequestInit];

    expect(url).toBe('http://proxy:8080/auth/me');
    expect(new Headers(init.headers).get('cookie')).toBe('inferon_session=abc');
  });

  it('sends no cookie header when there is no session', async () => {
    const fetchImpl = vi.fn(async () => json({}, 401));

    await expect(makeApi(fetchImpl).me()).rejects.toBeInstanceOf(InferonApiError);

    const [, init] = fetchImpl.mock.calls[0] as unknown as [string, RequestInit];

    expect(new Headers(init?.headers).has('cookie')).toBe(false);
  });
});

describe('InferonApi — error mapping', () => {
  it('maps the proxy JSON error envelope to a typed error', async () => {
    const fetchImpl = vi.fn(async () =>
      json({ error: { message: 'not signed in', type: 'unauthorized' } }, 401),
    );

    await expect(makeApi(fetchImpl).me()).rejects.toMatchObject({
      status: 401,
      type: 'unauthorized',
      message: 'not signed in',
    });
  });

  it('falls back to a generic type when the body is not JSON', async () => {
    const fetchImpl = vi.fn(async () => new Response('<html>502</html>', { status: 502 }));

    await expect(makeApi(fetchImpl).summary(7)).rejects.toMatchObject({
      status: 502,
      type: 'http_error',
    });
  });
});

describe('InferonApi — dashboard views', () => {
  it('builds the summary path with the window and keeps cost as a string', async () => {
    const fetchImpl = vi.fn(async () =>
      json({
        scope: 'tenant',
        tenant_id: 't1',
        window_days: 30,
        requests: 12,
        errors: 1,
        total_cost_usd: '0.000007',
        first_trace_at: '2026-10-01',
        last_trace_at: '2026-10-04',
      }),
    );
    const summary = await makeApi(fetchImpl).summary(30, 0, 'inferon_session=abc');

    expect(summary.total_cost_usd).toBe('0.000007');
    expect(typeof summary.total_cost_usd).toBe('string');

    const [url] = fetchImpl.mock.calls[0] as unknown as [string];

    expect(url).toBe('http://proxy:8080/v1/dashboard/api/summary?days=30');
  });

  it('adds an offset for the comparison window and parses the daily series', async () => {
    const fetchImpl = vi.fn<FetchLike>().mockResolvedValueOnce(
      json({
        window_days: 7,
        offset_days: 7,
        points: [
          { date: '2026-09-29', requests: 2, errors: 0, cost_usd: '0.001', prompt_tokens: 10, completion_tokens: 4, avg_ttft_ms: 120.5, avg_duration_ms: 900 },
        ],
      }),
    );
    const series = await makeApi(fetchImpl).timeseries(7, 7);

    expect(series.points[0]?.date).toBe('2026-09-29');
    expect(series.points[0]?.cost_usd).toBe('0.001');
    expect(series.points[0]?.avg_ttft_ms).toBe(120.5);
    expect(fetchImpl.mock.calls[0]?.[0]).toBe('http://proxy:8080/v1/dashboard/api/timeseries?days=7&offset=7');
  });

  it('parses the by-model, by-prompt and waste views', async () => {
    const fetchImpl = vi
      .fn<FetchLike>()
      .mockResolvedValueOnce(json({ models: [{ model: 'deepseek-chat', requests: 3, prompt_tokens: 10, completion_tokens: 5, cost_usd: '0.001' }] }))
      .mockResolvedValueOnce(json({ prompts: [{ system_prompt_hash: 'h1', requests: 2, prompt_tokens: 9, cost_usd: '0.002' }] }))
      .mockResolvedValueOnce(json({ cache_miss: { requests: 1, spend_usd: '0.01', savings_est_usd: '0.00' } }));

    const api = makeApi(fetchImpl);

    expect((await api.byModel(7)).models[0]?.cost_usd).toBe('0.001');
    expect((await api.byPrompt(7, 5)).prompts[0]?.system_prompt_hash).toBe('h1');
    expect((await api.waste(7)).cache_miss?.savings_est_usd).toBe('0.00');
    expect(fetchImpl.mock.calls[1]?.[0]).toBe('http://proxy:8080/v1/dashboard/api/by-prompt?days=7&limit=5');
  });
});

describe('InferonApi — tenant management', () => {
  it('lists providers with masked hints and posts a new one', async () => {
    const fetchImpl = vi
      .fn<FetchLike>()
      .mockResolvedValueOnce(
        json({
          providers: [
            { id: 'p1', kind: 'deepseek', name: 'Mine', base_url: 'https://api.deepseek.com/v1', key_hint: '\u2026ab12', created_at: '0' },
          ],
          kinds: ['deepseek', 'openai', 'openai-compatible'],
          role: 'owner',
        }),
      )
      .mockResolvedValueOnce(json({ id: 'p2' }));

    const api = makeApi(fetchImpl);
    const listed = await api.providers();

    expect(listed.role).toBe('owner');
    expect(listed.providers[0]?.key_hint).toBe('\u2026ab12');
    expect(fetchImpl.mock.calls[0]?.[0]).toBe('http://proxy:8080/v1/dashboard/api/providers');

    await api.createProvider({ kind: 'openai', base_url: 'https://api.openai.com/v1', key: 'sk-x' });
    const [url, init] = fetchImpl.mock.calls[1] as unknown as [string, RequestInit];

    expect(url).toBe('http://proxy:8080/v1/dashboard/api/providers');
    expect(JSON.parse(init.body as string)).toMatchObject({ kind: 'openai', key: 'sk-x' });
  });

  it('lists keys with the caller role and keeps metadata only', async () => {
    const fetchImpl = vi.fn(async () =>
      json({ role: 'owner', keys: [{ id: 'k1', label: 'ci', created_at: 'now', revoked: false, revoked_at: null, last_used_at: null }] }),
    );
    const keys = await makeApi(fetchImpl).keys('inferon_session=abc');

    expect(keys.role).toBe('owner');
    expect(keys.keys[0]?.label).toBe('ci');
    expect(JSON.stringify(keys)).not.toContain('sk-inferon-');
  });

  it('mints a key with a POST body and returns the raw key once', async () => {
    const fetchImpl = vi.fn(async () =>
      json({ key_id: 'k2', tenant_id: 't1', label: 'ci', key: 'sk-inferon-abc' }),
    );
    const created = await makeApi(fetchImpl).createKey('ci', 'inferon_session=abc');

    expect(created.key).toBe('sk-inferon-abc');

    const [url, init] = fetchImpl.mock.calls[0] as unknown as [string, RequestInit];

    expect(url).toBe('http://proxy:8080/v1/dashboard/api/keys');
    expect(init.method).toBe('POST');
    expect(init.body).toBe(JSON.stringify({ label: 'ci' }));
  });

  it('revokes a key by id and lists members with pending invites', async () => {
    const fetchImpl = vi
      .fn<FetchLike>()
      .mockResolvedValueOnce(json({ key_id: 'k2', revoked: true }))
      .mockResolvedValueOnce(json({ role: 'member', members: [{ id: 'u1', github_login: 'kane-mar', role: 'owner' }], invites: ['alice'] }));

    const api = makeApi(fetchImpl);
    const revoked = await api.revokeKey('k2', 'inferon_session=abc');
    const members = await api.members('inferon_session=abc');

    expect(revoked.revoked).toBe(true);
    expect(fetchImpl.mock.calls[0]?.[0]).toBe('http://proxy:8080/v1/dashboard/api/keys/k2/revoke');
    expect(members.invites).toEqual(['alice']);
  });
});

describe('InferonApi — operator views', () => {
  it('authenticates with the operator key when no session is available', async () => {
    const fetchImpl = vi.fn(async () => json({ tenants: [], window_days: 7 }));
    await makeApi(fetchImpl).operatorTenants(7, { operatorKey: 'sk-op' });

    const [url, init] = fetchImpl.mock.calls[0] as unknown as [string, RequestInit];

    expect(url).toBe('http://proxy:8080/v1/dashboard/api/operator/tenants?days=7');
    expect(new Headers(init.headers).get('authorization')).toBe('Bearer sk-op');
  });
});

describe('formatUsd — decimal strings, never floats', () => {
  it.each([
    ['0.000007', '$0.000007'],
    ['0.001234', '$0.001234'],
    ['0', '$0.00'],
    ['0.000000', '$0.00'],
    ['0.001230', '$0.00123'],
    ['12.000000', '$12'],
    ['', '$0.00'],
    ['1234.50', '$1,234.5'],
    ['-0.5', '-$0.5'],
    ['1.6e-5', '$1.6e-5'],
  ])('formats %s as %s', (input, expected) => {
    expect(formatUsd(input)).toBe(expected);
  });
});
