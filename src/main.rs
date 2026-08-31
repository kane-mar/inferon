use inferon::{build_router, parse_port, ping_health, AppState};
use reqwest::Client;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

const DEFAULT_UPSTREAM_URL: &str = "https://api.deepseek.com/v1/chat/completions";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let port = parse_port(std::env::var("PORT").ok().as_deref());

    if std::env::args().nth(1).as_deref() == Some("--healthcheck") {
        std::process::exit(if ping_health(port).await { 0 } else { 1 });
    }

    let state = Arc::new(AppState {
        http_client: Client::builder().pool_max_idle_per_host(100).build()?,
        upstream_url: std::env::var("UPSTREAM_URL")
            .unwrap_or_else(|_| DEFAULT_UPSTREAM_URL.to_string()),
        upstream_api_key: std::env::var("UPSTREAM_API_KEY")
            .unwrap_or_else(|_| "".to_string()),
    });

    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    info!("inferon listening on port {port}");

    axum::serve(listener, build_router(state)).await?;
    Ok(())
}
