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

    // ITEM-070 edge mode: run the proxy next to the customer's own model. The
    // cloud stays the control plane; the local provider credential is a secret
    // held only here.
    let argv: Vec<String> = std::env::args().collect();
    if argv.iter().any(|arg| arg == "--edge") {
        let flag = |name: &str| -> Option<String> {
            argv.iter()
                .position(|arg| arg == name)
                .and_then(|index| argv.get(index + 1))
                .cloned()
        };
        let credential = |name: &str| -> String {
            match inferon::edge_runtime::credential(
                flag(name),
                flag(&format!("{name}-file")),
                name,
            ) {
                Ok(value) => value,
                Err(message) => {
                    eprintln!("{message}");
                    std::process::exit(2);
                }
            }
        };
        let options = inferon::edge_runtime::EdgeOptions {
            control_url: flag("--control-url").unwrap_or_default(),
            node_token: credential("--node-token"),
            provider_url: flag("--provider-url").unwrap_or_default(),
            provider_key: credential("--provider-key"),
            provider_kind: flag("--provider-kind")
                .unwrap_or_else(|| "openai-compatible".to_string()),
            port,
            sync_interval: std::time::Duration::from_secs(30),
        };
        return inferon::edge_runtime::run(options).await;
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

    // Cost anomaly alerts (ITEM-020): one periodic evaluation pass; webhook
    // delivery is best-effort from a compose env var (no new infra, AC4).
    {
        let pool = pool.clone();
        let http_client = Client::new();
        let webhook_url = std::env::var("ALERT_WEBHOOK_URL")
            .ok()
            .filter(|u| !u.is_empty());
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(
                ALERT_EVAL_INTERVAL_SECS,
            ));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                inferon::alerts::run_evaluation(&pool, webhook_url.as_deref(), &http_client).await;
            }
        });
    }

    // Adaptive policy loop (ITEM-064): derive a bounded policy from telemetry
    // and publish it; the hot path reads it on the next request.
    {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut ticker =
                tokio::time::interval(std::time::Duration::from_secs(ADAPT_INTERVAL_SECS));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                inferon::policy::run_adaptation(&pool).await;
            }
        });
    }

    // ITEM-031 slice 1: drift detection -> evolution job queue.
    {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut ticker =
                tokio::time::interval(std::time::Duration::from_secs(EVOLUTION_INTERVAL_SECS));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                inferon::evolution::run_evolution(&pool).await;
            }
        });
    }

    // A connect timeout keeps an unreachable provider (e.g. a LAN model the
    // cloud host cannot route to) from hanging a request indefinitely; it
    // covers DNS + TCP + TLS. No overall timeout — streaming responses can be
    // long-lived.
    let http_client = Client::builder()
        .pool_max_idle_per_host(100)
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()?;
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

    // Provider credentials are encrypted at rest (ITEM-043). Without a master
    // key, provider provisioning is disabled — tenants keep using the shared key.
    let secrets = inferon::secret::Secrets::from_env().map(Arc::new);
    if secrets.is_none() {
        tracing::info!("provider provisioning disabled — set PROVIDER_ENCRYPTION_KEY (32-byte hex)");
    }

    let state = Arc::new(AppState {
        http_client,
        upstream_url: std::env::var("UPSTREAM_URL")
            .ok()
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| DEFAULT_UPSTREAM_URL.to_string()),
        tenant_store: Arc::new(PostgresTenantStore::new(pool.clone())),
        provider_store: Arc::new(inferon::providers::PostgresProviderStore::new(
            pool.clone(),
            secrets,
        )),
        model_store: Arc::new(inferon::models::PostgresModelStore::new(pool.clone())),
        routing_store: Arc::new(inferon::routing::PostgresRoutingStore::new(pool.clone())),
        budget_store: Arc::new(inferon::budget::PostgresBudgetStore::new(pool.clone())),
        affinity_store: std::sync::Arc::new(inferon::affinity::PostgresAffinityStore::new(
            pool.clone(),
        )),
        policy_store: Arc::new(inferon::policy::PostgresPolicyStore::new(pool.clone())),
        edge_store: Arc::new(inferon::edge::PostgresEdgeNodeStore::new(pool.clone())),
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
