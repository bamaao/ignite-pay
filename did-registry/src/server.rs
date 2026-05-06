use axum::routing::{get, post};
use axum::Router;

use crate::state::RegistryState;

/// Build the axum router with all REST API routes.
pub fn build_router(state: RegistryState) -> Router {
    #[allow(unused_mut)]
    let mut router = Router::new()
        .route("/health", get(health))
        // DID resolution
        .route("/v1/did/resolve/{did}", get(crate::handlers::resolve::resolve_did))
        // Auth
        .route("/v1/auth/nonce", get(crate::handlers::nonce::issue_nonce))
        // Merchant management
        .route("/v1/merchants/register", post(crate::handlers::register::register_merchant))
        .route("/v1/merchants/verify/{did}", get(crate::handlers::verify::verify_merchant))
        .route("/v1/merchants/rotate-key", post(crate::handlers::rotate_key::rotate_key))
        .route("/v1/merchants/status/{did}", get(crate::handlers::status::merchant_status))
        .route("/v1/merchants/update-vc", post(crate::handlers::update_vc::update_vc))
        .route("/v1/merchants/confirm", post(crate::handlers::confirm::confirm_register))
        // VC issuance & revocation
        .route("/v1/vc/issue", post(crate::handlers::issue_vc::issue_vc))
        .route("/v1/vc/revoke", post(crate::handlers::revoke_vc::revoke_vc))
        // Fee records
        .route("/v1/fees", get(crate::handlers::fees::list_fees));

    // ZK Compression proof endpoint (only in zk-compression mode)
    #[cfg(feature = "zk-compression")]
    {
        router = router.route("/v1/proof", post(crate::handlers::proof::get_proof));
    }

    router.with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
