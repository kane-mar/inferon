// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

use inferon::{
    auth::PostgresUserStore,
    build_router, load_system_prompt_counts, parse_port, ping_health, telemetry_worker,
    AppState, PostgresTenantStore, PostgresTraceSink, TraceMessage,
};
use reqwest::Client;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

const DEFAULT_UPSTREAM_URL: &str = "https://api.deepseek.com/v1/chat/completions";
const DEFAULT_DATABASE_URL: &str = "postgres://inferon:inferon@localhost:5432/inferon";
const TELEMETRY_CHANNEL_CAPACITY: usize = 10_000;
const TELEMETRY_FLUSH_INTERVAL_MS: u64 = 500;
const TELEMETRY_BATCH_SIZE: usize = 100;
/// Alert evaluation cadence (ITEM-020): an in-process interval task, no new
/// daemon/service — the proxy binary stays the only deployment unit.
const ALERT_EVAL_INTERVAL_SECS: u64 = 60;
/// Adaptive policy cadence (ITEM-064): one bounded analysis pass. The first
/// tick fires immediately, so a fresh deploy bootstraps a policy.
const ADAPT_INTERVAL_SECS: u64 = 900;
/// Evolution cadence (ITEM-031 slice 1): drift detection over the guard
/// telemetry, queueing a deduped job per drifting model.
const EVOLUTION_INTERVAL_SECS: u64 = 900;
/// Provider reachability cadence (ITEM-082): one bounded pass over due
/// providers so a dark on-prem endpoint shows unreachable before a tenant
/// request has to fail on it. Probing is a `/models` GET, never inference.
const PROVIDER_PROBE_INTERVAL_SECS: u64 = 300;
/// Payload retention cadence (ITEM-088): raw prompts/completions are purged past
/// each tenant's window. Costs and aggregates are never touched.
const RETENTION_INTERVAL_SECS: u64 = 6 * 60 * 60;
/// ITEM-089: embed and cluster the pending task texts. One embed is ~150 ms on
/// the production tier, so this runs off the request path on a slow cadence.
const SIGNATURE_INTERVAL_SECS: u64 = 300;
/// ITEM-091: compare pruned vs held-out quality and kill a policy that hurts.
const PRUNE_EVAL_INTERVAL_SECS: u64 = 900;

/// Run an async task forever on a fixed cadence. The first tick fires
/// immediately; missed ticks are skipped rather than burst-run, so a slow pass
/// does not queue back-to-back follow-ups.
fn spawn_periodic<F, Fut>(name: &'static str, period: std::time::Duration, task: F)
where
    F: Fn() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            tracing::debug!("periodic task: {name}");
            task().await;
        }
    });
}

/// The hot-path cache, or its kill switch (`HOTPATH_CACHE=off`): every read goes
/// to the store, so a cache problem can be taken out of the equation instantly.
fn hot_path() -> inferon::hotpath::HotPath {
    if std::env::var("HOTPATH_CACHE").is_ok_and(|value| value == "off") {
        tracing::warn!("hot-path cache disabled (HOTPATH_CACHE=off)");
        inferon::hotpath::HotPath::uncached()
    } else {
        inferon::hotpath::HotPath::new()
    }
}

/// Wire the periodic workers (ITEM-020/031/064/082). Each is one bounded pass on
/// a fixed cadence; the first tick fires immediately.
fn spawn_workers(
    pool: &sqlx::PgPool,
    http_client: &Client,
    provider_store: &Arc<inferon::providers::PostgresProviderStore>,
) {
    // Cost anomaly alerts (ITEM-020): one periodic evaluation pass; webhook
    // delivery is best-effort from a compose env var (no new infra, AC4).
    let webhook_url = std::env::var("ALERT_WEBHOOK_URL")
        .ok()
        .filter(|u| !u.is_empty());
    if webhook_url.is_none() {
        tracing::info!(
            "alert webhook not configured — findings are evaluated and stored, not delivered"
        );
    }
    let alert_client = Client::new();
    let alerts_pool = pool.clone();
    spawn_periodic(
        "alerts",
        std::time::Duration::from_secs(ALERT_EVAL_INTERVAL_SECS),
        move || {
            let pool = alerts_pool.clone();
            let client = alert_client.clone();
            let webhook = webhook_url.clone();
            async move {
                inferon::alerts::run_evaluation(&pool, webhook.as_deref(), &client).await;
            }
        },
    );

    // Adaptive policy loop (ITEM-064): derive a bounded policy from telemetry
    // and publish it; the hot path reads it on the next request.
    let adapt_pool = pool.clone();
    spawn_periodic(
        "adaptive-policy",
        std::time::Duration::from_secs(ADAPT_INTERVAL_SECS),
        move || {
            let pool = adapt_pool.clone();
            async move { inferon::policy::run_adaptation(&pool).await }
        },
    );

    // ITEM-031 slice 1: drift detection -> evolution job queue.
    let evolution_pool = pool.clone();
    spawn_periodic(
        "evolution",
        std::time::Duration::from_secs(EVOLUTION_INTERVAL_SECS),
        move || {
            let pool = evolution_pool.clone();
            async move {
                inferon::evolution::run_evolution(&pool).await;
            }
        },
    );

    // ITEM-089: label unclustered prompts with a semantic task signature. The
    // embedder is the same artifact the router uses; without it the pass
    // degrades to exact-hash labels instead of failing.
    let signature_pool = pool.clone();
    let signature_embedder = inferon::embed::from_env();
    spawn_periodic(
        "task-signatures",
        std::time::Duration::from_secs(SIGNATURE_INTERVAL_SECS),
        move || {
            let pool = signature_pool.clone();
            let embedder = signature_embedder.clone();
            async move {
                inferon::signatures::run_pass(&pool, embedder.as_ref()).await;
            }
        },
    );

    // ITEM-091: the pruning kill switch. Savings never outrank a worse answer.
    let prune_pool = pool.clone();
    spawn_periodic(
        "prune-evaluation",
        std::time::Duration::from_secs(PRUNE_EVAL_INTERVAL_SECS),
        move || {
            let pool = prune_pool.clone();
            async move {
                inferon::prune::evaluate(&pool).await;
            }
        },
    );

    // ITEM-082: refresh each provider's reachability on a cadence. The manual
    // Test-connection button stays; this is what flips a dark endpoint to
    // unreachable without waiting for a tenant request to fail.
    let probe_store = provider_store.clone();
    let probe_client = http_client.clone();
    // ITEM-088: payload retention, bounded batches.
    let retention_pool = pool.clone();
    spawn_periodic(
        "payload-retention",
        std::time::Duration::from_secs(RETENTION_INTERVAL_SECS),
        move || {
            let pool = retention_pool.clone();
            async move {
                match inferon::retention::purge_expired(&pool).await {
                    Ok(0) => {}
                    Ok(n) => tracing::info!("payload retention: purged {n} row(s)"),
                    Err(e) => tracing::warn!("payload retention failed: {e}"),
                }
            }
        },
    );

    spawn_periodic(
        "provider-reachability",
        std::time::Duration::from_secs(PROVIDER_PROBE_INTERVAL_SECS),
        move || {
            let store = probe_store.clone();
            let client = probe_client.clone();
            async move { inferon::providers::run_reachability_pass(&*store, &client).await }
        },
    );

}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let port = parse_port(std::env::var("PORT").ok().as_deref());

    if std::env::args().nth(1).as_deref() == Some("--healthcheck") {
        std::process::exit(if ping_health(port).await { 0 } else { 1 });
    }

    // ITEM-027 AC3 smoke harness: same binary, lying-upstream mock mode (no
    // postgres, no telemetry worker — just the chat endpoint). Keeps the
    // deployment unit a single image.
    if std::env::args().nth(1).as_deref() == Some("--mock-upstream") {
        inferon::mock_upstream::serve(port).await?;
        return Ok(());
    }

    // ITEM-036 AC4 smoke harness: mock GitHub OAuth (same single image).
    if std::env::args().nth(1).as_deref() == Some("--mock-github") {
        inferon::mock_github::serve(port).await?;
        return Ok(());
    }

    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string());
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    let (telemetry_tx, telemetry_rx) = tokio::sync::mpsc::channel::<TraceMessage>(TELEMETRY_CHANNEL_CAPACITY);
    let initial_counts = load_system_prompt_counts(&pool).await?;
    tokio::spawn(telemetry_worker(
        telemetry_rx,
        Box::new(PostgresTraceSink::new(pool.clone())),
        std::time::Duration::from_millis(TELEMETRY_FLUSH_INTERVAL_MS),
        TELEMETRY_BATCH_SIZE,
        initial_counts,
    ));

    // A connect timeout keeps an unreachable provider (e.g. a LAN model the
    // cloud host cannot route to) from hanging a request indefinitely; it
    // covers DNS + TCP + TLS. No overall timeout — streaming responses can be
    // long-lived.
    let http_client = Client::builder()
        .pool_max_idle_per_host(100)
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()?;
    // Periodic workers run off the same pool: alerts, the adaptive policy, the
    // evolution queue and provider reachability (the only one that needs a
    // store and the outbound client).
    let secrets = inferon::secret::Secrets::from_env().map(Arc::new);
    if secrets.is_none() {
        tracing::info!("provider provisioning disabled — set PROVIDER_ENCRYPTION_KEY (32-byte hex)");
    }
    let provider_store = Arc::new(inferon::providers::PostgresProviderStore::new(
        pool.clone(),
        secrets,
    ));
    spawn_workers(&pool, &http_client, &provider_store);

    // Human accounts (ITEM-035): `None` when GitHub OAuth is unconfigured —
    // /auth/login answers 503 then (fail closed on the login screen).
    let accounts = inferon::auth::Accounts::from_env(
        http_client.clone(),
        std::sync::Arc::new(PostgresUserStore::new(pool.clone())),
    );
    if accounts.is_none() {
        tracing::info!(
            "GitHub OAuth not configured (need GITHUB_CLIENT_ID, GITHUB_CLIENT_SECRET, SESSION_SECRET) — /auth/login answers 503"
        );
    }
    // ITEM-037 AC1: OPERATOR_GITHUB_LOGINS bootstrap — promote existing users
    // whose login is on the list (new sign-ins get flagged by the same list).
    if let Some(accounts) = &accounts {
        let logins: Vec<&str> = accounts.operator_logins.iter().map(String::as_str).collect();
        if !logins.is_empty() {
            match accounts.users.promote_logins(&logins).await {
                Ok(n) => tracing::info!("operator bootstrap: promoted {n} existing user(s)"),
                Err(e) => tracing::warn!("operator bootstrap failed: {e}"),
            }
        }
    }

    let state = Arc::new(AppState {
        http_client: http_client.clone(),
        upstream_url: std::env::var("UPSTREAM_URL")
            .ok()
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| DEFAULT_UPSTREAM_URL.to_string()),
        tenant_store: Arc::new(PostgresTenantStore::new(pool.clone())),
        provider_store: provider_store.clone(),
        model_store: Arc::new(inferon::models::PostgresModelStore::new(pool.clone())),
        routing_store: Arc::new(inferon::routing::PostgresRoutingStore::new(pool.clone())),
        budget_store: Arc::new(inferon::budget::PostgresBudgetStore::new(pool.clone())),
        hotpath: Arc::new(hot_path()),
        affinity_store: std::sync::Arc::new(inferon::affinity::PostgresAffinityStore::new(
            pool.clone(),
        )),
        policy_store: Arc::new(inferon::policy::PostgresPolicyStore::new(pool.clone())),
        config_audit: Arc::new(inferon::config_audit::PostgresConfigAuditStore::new(pool.clone())),
        telemetry_tx,
        operator_key: std::env::var("OPERATOR_API_KEY").ok().filter(|k| !k.is_empty()),
        accounts,
        db: pool,
    });

    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    info!("inferon listening on port {port}");

    axum::serve(listener, build_router(state)).await?;
    Ok(())
}
