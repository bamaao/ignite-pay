// Copyright (c) 2026 zouyc zouyccq@gmail.com.
// All rights reserved.
//
// Licensed under the Business Source License 1.1 (BSL 1.1).
// You may not use this file except in compliance with the License.
//
// Change Date: 2031-01-01
// On the Change Date, or the fourth anniversary of the first publicly available
// distribution of the code under the BSL, whichever comes first, the code
// automatically becomes available under the Apache License 2.0.

use didcomm_router::config::Config;
use didcomm_router::server;
use tracing::info;
use tracing_subscriber::fmt::writer::MakeWriterExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing with stderr + rolling file output
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "didcomm_router=info".into());

    // Ensure log directory exists
    std::fs::create_dir_all("logs").ok();
    let file_appender = tracing_appender::rolling::daily("logs", "router.log");
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr.and(file_appender))
        .with_env_filter(env_filter)
        .init();

    // Load config
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());
    let config = Config::load(&config_path)?;
    let bind_addr = config.bind_addr();

    info!(
        "Starting DIDComm Router on {}{}",
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
