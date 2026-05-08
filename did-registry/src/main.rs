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

use did_registry::config::Config;
use did_registry::server;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "did_registry=info".into()),
        )
        .init();

    // Load config
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());
    let config = Config::load(&config_path)?;
    let bind_addr = config.bind_addr();

    info!("Starting DID Registry on {}", bind_addr);

    // Build app state and router
    let state = did_registry::state::RegistryState::new(config)?;
    let app = server::build_router(state);

    // Start server
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    info!("Listening on {}", bind_addr);
    axum::serve(listener, app).await?;

    Ok(())
}
