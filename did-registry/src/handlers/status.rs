use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use tracing::info;

use crate::did::resolver::compute_did_hash;
use crate::state::RegistryState;

/// `GET /v1/merchants/status/{did}` — Check a merchant's on-chain DID status via PDA account.
pub async fn merchant_status(
    State(state): State<RegistryState>,
    Path(did): Path<String>,
) -> impl IntoResponse {
    if !did.starts_with("did:ignite:") {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": "Invalid DID format" })),
        )
            .into_response();
    }

    info!("Checking status for merchant: {}", did);

    let did_hash = compute_did_hash(&did);

    // Check cache
    if let Some(cached_did) = state.get_cached_merchant(&did_hash) {
        return (
            StatusCode::OK,
            axum::Json(serde_json::json!({
                "status": "active",
                "original_pubkey": cached_did.original_pk.to_string(),
                "controller_pubkey": cached_did.controller_pk.to_string(),
                "last_updated": cached_did.last_updated,
                "nonce": cached_did.nonce,
            })),
        )
            .into_response();
    }

    (
        StatusCode::NOT_FOUND,
        axum::Json(serde_json::json!({ "error": "Merchant not found" })),
    )
        .into_response()
}
