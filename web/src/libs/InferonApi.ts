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
  cached_prompt_tokens: number;
  /** Fraction of prompt tokens served from the provider cache (0..1), or null. */
  cache_hit_rate: number | null;
  cache_savings_usd: string;
  /** ITEM-058: model-substitution (router/fallback) savings attribution. */
  router_savings_usd: string;
  fallback_savings_usd: string;
  total_savings_usd: string;
  unreduced_cost_usd: string;
  /** ITEM-063: requests that cache affinity re-targeted. */
  cache_affinity_requests: number;
  truncated: number;
  truncated_rate: number | null;
  /** ITEM-057: failure counts by class (upstream_429, transport, ...). */
  errors_by_class: Record<string, number>;
  avg_ttft_ms: number | null;
  avg_duration_ms: number | null;
  /** ITEM-056: tail latency percentiles (ms), null when no samples. */
  ttft_p50_ms: number | null;
  ttft_p95_ms: number | null;
  ttft_p99_ms: number | null;
  duration_p50_ms: number | null;
  duration_p95_ms: number | null;
  duration_p99_ms: number | null;
  /** ITEM-066: the proxy's own pre-upstream time (p50/p95). */
  proxy_p50_ms: number | null;
  proxy_p95_ms: number | null;
  first_trace_at: string | null;
  last_trace_at: string | null;
};

export type ModelRow = {
  model: string;
  requests: number;
  prompt_tokens: number;
  completion_tokens: number;
  cost_usd: string;
  cached_prompt_tokens: number;
  cache_savings_usd: string;
  /** ITEM-058: what this model's traffic would have cost on the asked models. */
  savings_usd: string;
  frontier_cost_usd: string;
};

export type ByModel = { models: ModelRow[] };

/** ITEM-055: derived response-quality view. */
export type QualityOverall = {
  requests: number;
  ok: number;
  empty: number;
  refused: number;
  errors: number;
  truncated: number;
  unclassified: number;
  guard_fallback: number;
  thumbs_up: number;
  thumbs_down: number;
  success_rate: number | null;
  empty_rate: number | null;
  refusal_rate: number | null;
  error_rate: number | null;
  guard_fallback_rate: number | null;
  /** ITEM-057: aborted/cut-off responses and their rate. */
  truncated_rate: number | null;
  /** ITEM-057: failure counts by class (upstream_429, transport, ...). */
  errors_by_class: Record<string, number>;
};

export type QualityModelRow = {
  model: string;
  requests: number;
  ok: number;
  empty: number;
  errors: number;
  thumbs_up: number;
  thumbs_down: number;
  success_rate: number | null;
};

export type QualityPromptRow = {
  system_prompt_hash: string;
  requests: number;
  ok: number;
  empty: number;
  errors: number;
  success_rate: number | null;
};

export type Quality = {
  window_days: number;
  scope: 'tenant' | 'operator';
  overall: QualityOverall;
  models: QualityModelRow[];
  prompts: QualityPromptRow[];
};

/** ITEM-092: the performance report — proxy overhead first, then the rest. */
export type PerformanceCacheCount = {
  hits: number;
  misses: number;
  hit_rate: number | null;
};

export type PerformanceCache = {
  auth: PerformanceCacheCount;
  config: PerformanceCacheCount;
  route: PerformanceCacheCount;
  warm: PerformanceCacheCount;
  spend: PerformanceCacheCount;
  invalidations: number;
  tenants: number;
};

export type PerformanceModelRow = {
  model: string;
  requests: number;
  proxy_p50_us: number | null;
  proxy_p95_us: number | null;
  total_p50_ms: number | null;
  total_p95_ms: number | null;
};

export type PerformanceReport = {
  window: { days: number; offset_days: number };
  contract: {
    p50_us: number;
    p95_us: number;
    /** Below this many samples the report withholds a verdict. */
    min_samples: number;
    p50_met: boolean | null;
    p95_met: boolean | null;
  };
  overhead: {
    samples: number;
    under_1ms: number;
    under_1ms_rate: number | null;
    p50_us: number | null;
    p95_us: number | null;
    p99_us: number | null;
  };
  latency: {
    total_p50_ms: number | null;
    total_p95_ms: number | null;
    total_p99_ms: number | null;
    ttft_p50_ms: number | null;
    ttft_p95_ms: number | null;
    proxy_share: number | null;
  };
  /** Process-wide, operator-only; `null` for a tenant reader. */
  cache: PerformanceCache | null;
  by_model: PerformanceModelRow[];
};

export type PromptRow = {
  system_prompt_hash: string;
  requests: number;
  prompt_tokens: number;
  /** ITEM-063: cache hit rate for this prompt cluster (the affinity group). */
  cached_prompt_tokens: number;
  cache_savings_usd: string;
  cache_hit_rate: number | null;
  cost_usd: string;
};

export type ByPrompt = { prompts: PromptRow[] };

/** ITEM-065: an enrolled edge node (cloud control plane). */
export type EdgeNode = {
  id: string;
  name: string;
  last_seen_at: string | null;
  created_at: string;
};

export type EdgeNodes = { nodes: EdgeNode[] };

/** ITEM-047: spend grouped by provider (period-comparable via `offset`). */
export type ProviderSpendRow = {
  provider_id: string | null;
  name: string;
  kind: string;
  requests: number;
  /** ITEM-057/061: failures + rate, and latency (mean + p95), per provider. */
  errors: number;
  error_rate: number | null;
  avg_ttft_ms: number | null;
  ttft_p95_ms: number | null;
  avg_duration_ms: number | null;
  duration_p95_ms: number | null;
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

/** A read-only cost-audit share link (the token is shown only at creation). */
export type AuditShare = {
  id: string;
  label: string;
  created_at: string;
  last_viewed_at: string | null;
  revoked_at: string | null;
};

export type AuditShares = { shares: AuditShare[] };

/** The public read model behind a shared audit link. */
export type PublicAudit = {
  window_days: number;
  summary: {
    requests: number;
    errors: number;
    total_cost_usd: string;
    total_savings_usd: string;
    router_savings_usd: string;
    fallback_savings_usd: string;
    cache_savings_usd: string;
    truncated: number;
    cache_affinity_requests: number;
  };
  waste: Record<string, WasteFlag>;
  top: {
    model: { name: string; cost_usd: string } | null;
    provider: { name: string; cost_usd: string } | null;
    key: { name: string; cost_usd: string } | null;
  };
  quality: {
    error_rate: number | null;
    truncated_rate: number | null;
    guard_fallback_rate: number | null;
    errors_by_class: Record<string, number>;
  };
};

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
  /** The tenant's default provider (fallback for models no kind owns). */
  is_default: boolean;
  /** ITEM-051: last Test-connection result (null = never checked). */
  last_check_at: string | null;
  last_check_ok: boolean | null;
  created_at: string;
  updated_at: string;
};

export type ProvidersResponse = {
  providers: ProviderRow[];
  kinds: string[];
  role: Role;
};

/** Result of probing an unsaved provider config (ITEM-051). */
export type ProviderProbeResult = { ok: boolean; models: string[]; error?: string };

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

export type ProviderTest = {
  ok: boolean;
  status: number | null;
  /** ITEM-061/UX: models discovered by a successful probe. */
  models?: number;
  /** Diagnosis when the probe failed (upstream status or transport error). */
  error?: string | null;
};

/** A model a provider advertises, with that provider's flags + price. */
export type ProviderModel = {
  id: string;
  enabled: boolean;
  is_default: boolean;
  price: {
    input_usd_per_million: string;
    output_usd_per_million: string;
    /** False when the cost engine has no price row for this id. */
    priced: boolean;
    /** Served by a `local` provider: always $0 (ITEM-077/082). */
    local?: boolean;
  };
};

export type ProviderModelsResponse = { models: ProviderModel[] };

/** A provider with its cached advertised models (the tenant-wide list). */
export type ProviderModelsGroup = {
  id: string;
  name: string;
  kind: string;
  is_default: boolean;
  models: ProviderModel[];
};

export type ModelsResponse = { providers: ProviderModelsGroup[]; role: Role };

export type TierPair = { frontier: string; cheap: string };
export type ActiveTierPair = TierPair & { active: boolean };

export type RoutingResponse = {
  fallbacks: string[];
  tier_pairs: ActiveTierPair[];
  role: Role;
};

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

/** ITEM-062: operator engagement view — active tenants/keys, funnel, WoW. */
export type OperatorActivation = {
  window_days: number;
  active_tenants: number;
  active_keys: number;
  requests: number;
  requests_per_active_tenant: number | null;
  requests_per_active_key: number | null;
  signups: number;
  previous_signups: number;
  previous_active_tenants: number;
  previous_active_keys: number;
  previous_requests: number;
  active_tenants_wow: number | null;
  requests_wow: number | null;
  signups_wow: number | null;
  funnel: {
    signed_up: number;
    created_provider: number;
    first_request: number;
    returned: number;
  };
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
  /** ITEM-071: the provider's name for a `provider_error`/`provider_latency` alert. */
  provider_name?: string | null;
};

export type OperatorAlerts = { alerts: OperatorAlertRow[]; window_days: number };

/** ITEM-031: a drift-driven evolution job (queued -> ... -> promoted/failed). */
export type OperatorEvolutionJob = {
  id: string;
  tenant_id: string;
  adapter: string;
  reason: string;
  state: string;
  evidence: {
    requests?: number;
    fallbacks?: number;
    rate?: number;
    window_hours?: number;
  };
  created_at: string;
  updated_at: string;
  finished_at: string | null;
};

export type OperatorEvolution = { jobs: OperatorEvolutionJob[] };

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

  performance(days: number, cookie?: string): Promise<PerformanceReport> {
    return this.request<PerformanceReport>(
      'GET',
      `/v1/dashboard/api/performance${windowQuery(days)}`,
      undefined,
      { cookie },
    );
  }

  quality(days: number, cookie?: string): Promise<Quality> {
    return this.request<Quality>('GET', `/v1/dashboard/api/quality${windowQuery(days)}`, undefined, { cookie });
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

  edgeNodes(cookie?: string): Promise<EdgeNodes> {
    return this.request<EdgeNodes>('GET', '/v1/dashboard/api/edge/nodes', undefined, { cookie });
  }

  createEdgeNode(name: string, cookie?: string): Promise<{ id: string; token: string }> {
    return this.request('POST', '/v1/dashboard/api/edge/nodes', { name }, { cookie });
  }

  revokeEdgeNode(id: string, cookie?: string): Promise<{ removed: boolean }> {
    return this.request('POST', `/v1/dashboard/api/edge/nodes/${id}/revoke`, {}, { cookie });
  }

  auditShares(cookie?: string): Promise<AuditShares> {
    return this.request<AuditShares>('GET', '/v1/dashboard/api/audit/shares', undefined, { cookie });
  }

  createAuditShare(cookie?: string): Promise<{ id: string; token: string }> {
    return this.request('POST', '/v1/dashboard/api/audit/shares', { label: '' }, { cookie });
  }

  revokeAuditShare(id: string, cookie?: string): Promise<{ id: string; revoked: boolean }> {
    return this.request('POST', `/v1/dashboard/api/audit/shares/${id}/revoke`, {}, { cookie });
  }

  publicAudit(token: string, days = 30): Promise<PublicAudit> {
    return this.request<PublicAudit>('GET', `/v1/public/audit/${token}?days=${days}`);
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

  testProviderDraft(
    body: { kind: string; base_url: string; key: string; metadata?: ProviderMetadata },
    cookie?: string,
  ): Promise<ProviderProbeResult> {
    return this.request<ProviderProbeResult>('POST', '/v1/dashboard/api/providers/test', body, { cookie });
  }

  models(cookie?: string): Promise<ModelsResponse> {
    return this.request<ModelsResponse>('GET', '/v1/dashboard/api/models', undefined, { cookie });
  }

  providerModels(providerId: string, cookie?: string): Promise<ProviderModelsResponse> {
    return this.request<ProviderModelsResponse>(
      'GET',
      `/v1/dashboard/api/providers/${providerId}/models`,
      undefined,
      { cookie },
    );
  }

  setDefaultProvider(
    providerId: string,
    cookie?: string,
  ): Promise<{ provider_id: string; is_default: boolean }> {
    return this.request('POST', `/v1/dashboard/api/providers/${providerId}/default`, {}, { cookie });
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

  setTierPairs(pairs: TierPair[], cookie?: string): Promise<{ tier_pairs: ActiveTierPair[] }> {
    return this.request('POST', '/v1/dashboard/api/routing/tier-pairs', { tier_pairs: pairs }, { cookie });
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

  operatorActivation(days: number, auth: Auth): Promise<OperatorActivation> {
    return this.request('GET', `/v1/dashboard/api/operator/activation${windowQuery(days)}`, undefined, auth);
  }

  operatorEvolution(auth: Auth): Promise<OperatorEvolution> {
    return this.request('GET', '/v1/dashboard/api/operator/evolution', undefined, auth);
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
