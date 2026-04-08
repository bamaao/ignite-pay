use axum::routing::{get, post};
use axum::Router;

use crate::state::AppState;
use crate::transport;

/// Build the axum router with all routes.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/ws", get(transport::ws::ws_handler))
        .route("/", post(transport::http::post_message))
        .route("/health", get(health))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
