/**
 * Typed client for the Inferon data plane — the Rust proxy's JSON APIs.
 *
 * The web app never verifies sessions itself: server-side calls forward the
 * browser's `Cookie` header; client-side calls are same-origin (`baseUrl` '').
 * Failures keep the proxy's `{ error: { type, message } }` envelope in a typed
 * `InferonApiError`.
 *
 * Monetary values stay **strings** end to end (the proxy renders decimals as
 * strings so no float ever touches a cost — ITEM-019 AC3); format them with
 * `formatUsd`, never `parseFloat`.
 */

export type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

export class InferonApiError extends Error {
  readonly status: number;
  readonly type: string;

  constructor(status: number, type: string, message: string) {
    super(message);
    this.name = 'InferonApiError';
    this.status = status;
    this.type = type;
  }
}

export type Auth = { cookie?: string; operatorKey?: string };

export type Me = {
  id: string;
  github_id: number;
  github_login: string;
  is_operator: boolean;
};

export type Summary = {
  scope: 'tenant' | 'operator';
  tenant_id: string | null;
  window_days: number;
  offset_days: number;
  requests: number;
  errors: number;
  total_cost_usd: string;
  avg_cost_usd: string;
  total_prompt_tokens: number;
  total_completion_tokens: number;
  avg_ttft_ms: number | null;
  avg_duration_ms: number | null;
  first_trace_at: string | null;
  last_trace_at: string | null;
};

export type ModelRow = {
  model: string;
  requests: number;
  prompt_tokens: number;
  completion_tokens: number;
  cost_usd: string;
};

export type ByModel = { models: ModelRow[] };

export type PromptRow = {
  system_prompt_hash: string;
  requests: number;
  prompt_tokens: number;
  cost_usd: string;
};

export type ByPrompt = { prompts: PromptRow[] };

/** ITEM-047: spend grouped by provider (period-comparable via `offset`). */
export type ProviderSpendRow = {
  provider_id: string | null;
  name: string;
  kind: string;
  requests: number;
  prompt_tokens: number;
  completion_tokens: number;
  cost_usd: string;
  spend_limit_cents: number;
};

export type ByProvider = {
  window_days: number;
  offset_days: number;
  providers: ProviderSpendRow[];
};

/** ITEM-047: spend grouped by API key. */
export type KeySpendRow = {
  key_id: string | null;
  label: string;
  requests: number;
  prompt_tokens: number;
  completion_tokens: number;
  cost_usd: string;
  spend_limit_cents: number;
};

export type ByKey = { window_days: number; offset_days: number; keys: KeySpendRow[] };

export type WasteFlag = {
  requests: number;
  spend_usd: string;
  savings_est_usd: string;
};

export type Waste = Record<string, WasteFlag>;

export type TimeseriesPoint = {
  /** `YYYY-MM-DD` (server-side daily bucket). */
  date: string;
  requests: number;
  errors: number;
  cost_usd: string;
  prompt_tokens: number;
  completion_tokens: number;
  avg_ttft_ms: number | null;
  avg_duration_ms: number | null;
};

export type Timeseries = {
  window_days: number;
  offset_days: number;
  points: TimeseriesPoint[];
};

export type KeyRow = {
  id: string;
  label: string;
  created_at: string;
  revoked: boolean;
  revoked_at: string | null;
  last_used_at: string | null;
  /** ITEM-047: monthly cap in cents; 0 = no limit. */
  spend_limit_cents: number;
};

export type Role = 'owner' | 'member';
export type KeysResponse = { role: Role; keys: KeyRow[] };

export type MemberRow = { id: string; github_login: string; role: Role };
export type MembersResponse = { role: Role; members: MemberRow[]; invites: string[] };

export type ProviderMetadata = {
  /** OpenAI organization id. */
  org?: string;
  /** Extra upstream headers (e.g. `anthropic-version`). */
  headers?: Record<string, string>;
};

export type ProviderRow = {
  id: string;
  kind: string;
  name: string;
  base_url: string;
  /** Masked, e.g. `…ab12` — the key itself never leaves the backend. */
  key_hint: string;
  metadata: ProviderMetadata;
  /** ITEM-047: monthly cap in cents; 0 = no limit. */
  spend_limit_cents: number;
  /** The provider's catalog models with this provider's selection flags. */
  models: ProviderModel[];
  created_at: string;
  updated_at: string;
};

export type ProvidersResponse = { providers: ProviderRow[]; kinds: string[]; role: Role };

export type NewProvider = {
  kind: string;
  name?: string;
  base_url: string;
  key: string;
  metadata?: ProviderMetadata;
  spend_limit_cents?: number;
};

export type UpdateProvider = {
  name?: string;
  base_url?: string;
  /** Omit to keep the stored key; provide to rotate it. */
  key?: string;
  metadata?: ProviderMetadata;
  spend_limit_cents?: number;
};

export type ProviderTest = { ok: boolean; status: number | null };

export type ModelPrice = {
  input_usd_per_million: string;
  cached_input_usd_per_million: string | null;
  output_usd_per_million: string;
};

/** A model of one provider, merged with that provider's selection flags. */
export type ProviderModel = {
  id: string;
  name: string;
  context_window: number;
  max_output: number;
  reasoning: boolean;
  images: boolean;
  enabled: boolean;
  is_default: boolean;
  price: ModelPrice;
};

export type RoutingResponse = { fallbacks: string[]; role: Role };

export type TenantConfig = {
  budget_limit_cents: number;
  current_spend_cents: number;
  cheap_fallback_model: string | null;
  guard_enabled: boolean;
  router_enabled: boolean;
  router_threshold: number;
  role: Role;
};

export type ConfigUpdate = {
  budget_limit_cents: number;
  cheap_fallback_model: string | null;
  guard_enabled: boolean;
  router_enabled: boolean;
  router_threshold: number;
};

export type CreatedKey = {
  key_id: string;
  tenant_id: string;
  label: string;
  /** Shown exactly once — only the hash is stored server-side. */
  key: string;
};

export type OperatorTenantRow = {
  tenant_id: string;
  requests: number;
  errors: number;
  cost_usd: string;
};

export type OperatorTenants = { tenants: OperatorTenantRow[]; window_days: number };

export type OperatorGuard = {
  guarded_requests: number;
  fallback_requests: number;
  fallback_failed: number;
  by_reason: { reason: string; events: number }[];
  window_days: number;
};

export type OperatorAlertRow = {
  tenant_id: string;
  class: string;
  status: string;
  detail: unknown;
  threshold: unknown;
  first_fired_at: string;
  last_fired_at: string;
  cleared_at: string | null;
};

export type OperatorAlerts = { alerts: OperatorAlertRow[]; window_days: number };

type Options = { baseUrl?: string; fetchImpl?: FetchLike };

const windowQuery = (days: number, extra: Record<string, number> = {}): string => {
  const params = new URLSearchParams({ days: String(days) });
  for (const [key, value] of Object.entries(extra)) {
    params.set(key, String(value));
  }
  return `?${params.toString()}`;
};

export class InferonApi {
  private readonly baseUrl: string;
  private readonly fetchImpl: FetchLike;

  constructor(options: Options = {}) {
    this.baseUrl = options.baseUrl ?? '';
    this.fetchImpl = options.fetchImpl ?? ((input, init) => fetch(input, init));
  }

  /**
   * Server-side factory: the compose network address of the Rust proxy.
   * Client-side use `sameOrigin()` instead.
   */
  static fromEnv(env: Record<string, string | undefined> = process.env): InferonApi {
    return new InferonApi({ baseUrl: env.INFERON_API_URL ?? 'http://proxy:8080' });
  }

  /** Client-side factory: relative URLs against the public origin. */
  static sameOrigin(): InferonApi {
    return new InferonApi({ baseUrl: '' });
  }

  me(auth?: string): Promise<Me> {
    return this.request<Me>('GET', '/auth/me', undefined, { cookie: auth });
  }

  summary(days: number, offset = 0, cookie?: string): Promise<Summary> {
    return this.request<Summary>(
      'GET',
      `/v1/dashboard/api/summary${windowQuery(days, offset ? { offset } : {})}`,
      undefined,
      { cookie },
    );
  }

  /**
   * Daily buckets over the window, gap-filled server-side. `offset` shifts the
   * window back by that many days — pass `offset: days` for the prior period.
   */
  timeseries(days: number, offset = 0, cookie?: string): Promise<Timeseries> {
    return this.request<Timeseries>(
      'GET',
      `/v1/dashboard/api/timeseries${windowQuery(days, offset ? { offset } : {})}`,
      undefined,
      { cookie },
    );
  }

  byModel(days: number, cookie?: string): Promise<ByModel> {
    return this.request<ByModel>('GET', `/v1/dashboard/api/by-model${windowQuery(days)}`, undefined, { cookie });
  }

  byPrompt(days: number, limit = 10, cookie?: string): Promise<ByPrompt> {
    return this.request<ByPrompt>(
      'GET',
      `/v1/dashboard/api/by-prompt${windowQuery(days, { limit })}`,
      undefined,
      { cookie },
    );
  }

  byProvider(days: number, offset = 0, cookie?: string): Promise<ByProvider> {
    return this.request<ByProvider>(
      'GET',
      `/v1/dashboard/api/by-provider${windowQuery(days, offset ? { offset } : {})}`,
      undefined,
      { cookie },
    );
  }

  byKey(days: number, offset = 0, cookie?: string): Promise<ByKey> {
    return this.request<ByKey>(
      'GET',
      `/v1/dashboard/api/by-key${windowQuery(days, offset ? { offset } : {})}`,
      undefined,
      { cookie },
    );
  }

  waste(days: number, cookie?: string): Promise<Waste> {
    return this.request<Waste>('GET', `/v1/dashboard/api/waste${windowQuery(days)}`, undefined, { cookie });
  }

  keys(cookie?: string): Promise<KeysResponse> {
    return this.request<KeysResponse>('GET', '/v1/dashboard/api/keys', undefined, { cookie });
  }

  createKey(label: string, cookie?: string): Promise<CreatedKey> {
    return this.request<CreatedKey>('POST', '/v1/dashboard/api/keys', { label }, { cookie });
  }

  revokeKey(keyId: string, cookie?: string): Promise<{ key_id: string; revoked: boolean }> {
    return this.request('POST', `/v1/dashboard/api/keys/${keyId}/revoke`, {}, { cookie });
  }

  setKeyLimit(
    keyId: string,
    spendLimitCents: number,
    cookie?: string,
  ): Promise<{ key_id: string; spend_limit_cents: number }> {
    return this.request('POST', `/v1/dashboard/api/keys/${keyId}/limit`, { spend_limit_cents: spendLimitCents }, { cookie });
  }

  members(cookie?: string): Promise<MembersResponse> {
    return this.request<MembersResponse>('GET', '/v1/dashboard/api/members', undefined, { cookie });
  }

  inviteMember(githubLogin: string, cookie?: string): Promise<{ github_login: string; invited: boolean }> {
    return this.request('POST', '/v1/dashboard/api/members', { github_login: githubLogin }, { cookie });
  }

  removeMember(userId: string, cookie?: string): Promise<{ user_id: string; removed: boolean }> {
    return this.request('POST', `/v1/dashboard/api/members/${userId}/remove`, {}, { cookie });
  }

  providers(cookie?: string): Promise<ProvidersResponse> {
    return this.request<ProvidersResponse>('GET', '/v1/dashboard/api/providers', undefined, { cookie });
  }

  createProvider(body: NewProvider, cookie?: string): Promise<{ id: string }> {
    return this.request('POST', '/v1/dashboard/api/providers', body, { cookie });
  }

  updateProvider(providerId: string, body: UpdateProvider, cookie?: string): Promise<{ provider_id: string; updated: boolean }> {
    return this.request('POST', `/v1/dashboard/api/providers/${providerId}`, body, { cookie });
  }

  removeProvider(providerId: string, cookie?: string): Promise<{ provider_id: string; removed: boolean }> {
    return this.request('POST', `/v1/dashboard/api/providers/${providerId}/remove`, {}, { cookie });
  }

  testProvider(providerId: string, cookie?: string): Promise<ProviderTest> {
    return this.request<ProviderTest>('POST', `/v1/dashboard/api/providers/${providerId}/test`, {}, { cookie });
  }

  updateProviderModel(
    providerId: string,
    modelId: string,
    body: { enabled?: boolean; is_default?: boolean },
    cookie?: string,
  ): Promise<{ provider_id: string; model_id: string }> {
    return this.request(
      'POST',
      `/v1/dashboard/api/providers/${providerId}/models/${encodeURIComponent(modelId)}`,
      body,
      { cookie },
    );
  }

  routing(cookie?: string): Promise<RoutingResponse> {
    return this.request<RoutingResponse>('GET', '/v1/dashboard/api/routing', undefined, { cookie });
  }

  setRouting(fallbacks: string[], cookie?: string): Promise<{ fallbacks: string[] }> {
    return this.request('POST', '/v1/dashboard/api/routing', { fallbacks }, { cookie });
  }

  config(cookie?: string): Promise<TenantConfig> {
    return this.request<TenantConfig>('GET', '/v1/dashboard/api/config', undefined, { cookie });
  }

  updateConfig(body: ConfigUpdate, cookie?: string): Promise<{ updated: boolean }> {
    return this.request('POST', '/v1/dashboard/api/config', body, { cookie });
  }

  operatorSummary(days: number, auth: Auth): Promise<Summary> {
    return this.request('GET', `/v1/dashboard/api/operator/summary${windowQuery(days)}`, undefined, auth);
  }

  operatorTenants(days: number, auth: Auth): Promise<OperatorTenants> {
    return this.request('GET', `/v1/dashboard/api/operator/tenants${windowQuery(days)}`, undefined, auth);
  }

  operatorGuard(days: number, auth: Auth): Promise<OperatorGuard> {
    return this.request('GET', `/v1/dashboard/api/operator/guard${windowQuery(days)}`, undefined, auth);
  }

  operatorAlerts(days: number, auth: Auth): Promise<OperatorAlerts> {
    return this.request('GET', `/v1/dashboard/api/operator/alerts${windowQuery(days)}`, undefined, auth);
  }

  private async request<T>(
    method: 'GET' | 'POST',
    path: string,
    body?: unknown,
    auth: Auth = {},
  ): Promise<T> {
    const headers = new Headers({ accept: 'application/json' });
    if (body !== undefined) {
      headers.set('content-type', 'application/json');
    }
    if (auth.cookie) {
      headers.set('cookie', auth.cookie);
    }
    if (auth.operatorKey) {
      headers.set('authorization', `Bearer ${auth.operatorKey}`);
    }

    const response = await this.fetchImpl(`${this.baseUrl}${path}`, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      cache: 'no-store',
    });
    const payload: unknown = await response.json().catch(() => null);

    if (!response.ok) {
      const envelope = (payload as { error?: { type?: string; message?: string } } | null)?.error;
      throw new InferonApiError(
        response.status,
        envelope?.type ?? 'http_error',
        envelope?.message ?? `HTTP ${response.status}`,
      );
    }
    return payload as T;
  }
}

/**
 * Render a decimal cost string (or cents-style string) in the UI without ever
 * going through a float. Mirrors the dashboard shell's `usd()` helper.
 */
export const formatUsd = (value: string): string => {
  const negative = value.startsWith('-');
  const unsigned = negative ? value.slice(1) : value;
  const [rawWhole = '', rawFraction] = unsigned.split('.');
  const whole = rawWhole === '' ? '0' : rawWhole;
  const fraction = (rawFraction ?? '').replace(/0+$/, '');

  if (whole === '0' && fraction === '') {
    return `${negative ? '-' : ''}$0.00`;
  }
  // Thousands separators on the integer part only — never inside the fraction.
  const grouped = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  return `${negative ? '-' : ''}$${grouped}${fraction ? `.${fraction}` : ''}`;
};
