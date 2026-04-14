use axum::routing::{get, post};
use axum::Router;

use crate::state::RegistryState;

/// Build the axum router with all REST API routes.
pub fn build_router(state: RegistryState) -> Router {
    Router::new()
        .route("/health", get(health))
        // DID resolution
        .route("/v1/did/resolve/{did}", get(crate::handlers::resolve::resolve_did))
        // Merchant management
        .route("/v1/merchants/register", post(crate::handlers::register::register_merchant))
        .route("/v1/merchants/verify/{did}", get(crate::handlers::verify::verify_merchant))
        .route("/v1/merchants/rotate-key", post(crate::handlers::rotate_key::rotate_key))
        .route("/v1/merchants/status/{did}", get(crate::handlers::status::merchant_status))
        .route("/v1/merchants/update-vc", post(crate::handlers::update_vc::update_vc))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
