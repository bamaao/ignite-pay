use ignite_pay_channel_service::config::Role;
use ignite_pay_channel_service::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let config_path = args.get(1).map(|s| s.as_str()).unwrap_or("config-hub.toml");

    tracing::info!("Loading config from {}", config_path);
    let config = ignite_pay_channel_service::config::Config::load(config_path)?;
    let bind_addr = config.bind_addr();

    let state = AppState::new(config, Role::Hub)?;
    let app = ignite_pay_channel_service::server::router::build_router(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("Channel Hub service listening on {}", bind_addr);

    axum::serve(listener, app).await?;
    Ok(())
}
