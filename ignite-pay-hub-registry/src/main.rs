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

mod config;
mod error;
mod handlers;
mod models;
mod repository;
mod state;

use axum::routing::{delete, get, post, put};
use axum::Router;
use tower_http::cors::CorsLayer;

use crate::config::Config;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let config_path = args.get(1).map(|s| s.as_str()).unwrap_or("hub-registry.toml");

    tracing::info!("Loading config from {}", config_path);
    let config = Config::load(config_path)?;

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database.url)
        .await?;

    // Run migrations (idempotent — IF NOT EXISTS)
    sqlx::raw_sql(include_str!("../migrations/001_init.sql"))
        .execute(&pool)
        .await
        .ok(); // Ignore errors if tables already owned by another user

    tracing::info!("Database connected and migrations applied");

    let state = AppState::new(pool);
    let bind_addr = config.bind_addr();

    let app = Router::new()
        .route("/health", get(handlers::health))
        .route("/v1/hubs", post(handlers::register_hub))
        .route("/v1/hubs", get(handlers::list_hubs))
        .route("/v1/hubs/{hub_id}", get(handlers::get_hub))
        .route("/v1/hubs/{hub_id}", put(handlers::update_hub))
        .route("/v1/hubs/{hub_id}", delete(handlers::deregister_hub))
        .route("/v1/hubs/{hub_id}/metrics", get(handlers::get_hub_metrics))
        .route("/v1/hubs/{hub_id}/metrics", put(handlers::update_metrics))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("Hub Registry service listening on {}", bind_addr);

    axum::serve(listener, app).await?;
    Ok(())
}
