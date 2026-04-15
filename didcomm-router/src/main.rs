use didcomm_router::config::Config;
use didcomm_router::server;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "didcomm_router=info".into()),
        )
        .init();

    // Load config
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());
    let config = Config::load(&config_path)?;
    let bind_addr = config.bind_addr();

    info!(
        "Starting DIDComm Router [{}] on {}{}",
        config.router.did,
        bind_addr,
        if config.tls.is_enabled() { " (TLS)" } else { "" }
    );

    // Build app state and router
    let state = didcomm_router::state::RouterState::new(config.clone())?;
    let app = server::build_router(state);

    if config.tls.is_enabled() {
        let cert_path = config.tls.cert_path.as_deref().unwrap();
        let key_path = config.tls.key_path.as_deref().unwrap();
        let config = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path)
            .await?;
        axum_server::bind_rustls(bind_addr.parse()?, config)
            .serve(app.into_make_service())
            .await?;
    } else {
        let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
        info!("Listening on {}", bind_addr);
        axum::serve(listener, app).await?;
    }

    Ok(())
}
