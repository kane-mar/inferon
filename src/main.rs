use inferon::{
    build_router, parse_port, ping_health, telemetry_worker, AppState, PostgresTenantStore,
    PostgresTraceSink, TraceMessage,
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let port = parse_port(std::env::var("PORT").ok().as_deref());

    if std::env::args().nth(1).as_deref() == Some("--healthcheck") {
        std::process::exit(if ping_health(port).await { 0 } else { 1 });
    }

    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string());
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    let (telemetry_tx, telemetry_rx) = tokio::sync::mpsc::channel::<TraceMessage>(TELEMETRY_CHANNEL_CAPACITY);
    tokio::spawn(telemetry_worker(
        telemetry_rx,
        Box::new(PostgresTraceSink::new(pool.clone())),
        std::time::Duration::from_millis(TELEMETRY_FLUSH_INTERVAL_MS),
        TELEMETRY_BATCH_SIZE,
    ));

    let state = Arc::new(AppState {
        http_client: Client::builder().pool_max_idle_per_host(100).build()?,
        upstream_url: std::env::var("UPSTREAM_URL")
            .unwrap_or_else(|_| DEFAULT_UPSTREAM_URL.to_string()),
        tenant_store: Arc::new(PostgresTenantStore::new(pool)),
        telemetry_tx,
    });

    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    info!("inferon listening on port {port}");

    axum::serve(listener, build_router(state)).await?;
    Ok(())
}
