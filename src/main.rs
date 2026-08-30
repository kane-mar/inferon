use inferon::{build_router, parse_port, ping_health};
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let port = parse_port(std::env::var("PORT").ok().as_deref());

    if std::env::args().nth(1).as_deref() == Some("--healthcheck") {
        std::process::exit(if ping_health(port).await { 0 } else { 1 });
    }

    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    info!("inferon listening on port {port}");

    axum::serve(listener, build_router()).await?;
    Ok(())
}
