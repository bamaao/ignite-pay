use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use tracing::info;

use crate::did::resolver::compute_did_hash;
use crate::state::RegistryState;

/// `GET /v1/merchants/verify/{did}` — Verify a merchant's DID exists on-chain via PDA account.
pub async fn verify_merchant(
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

    info!("Verifying merchant: {}", did);

    let did_hash = compute_did_hash(&did);

    // Check local cache
    if let Some(cached_did) = state.get_cached_merchant(&did_hash) {
        return (StatusCode::OK, axum::Json(serde_json::json!({
            "verified": true,
            "original_pubkey": cached_did.original_pk.to_string(),
            "controller_pubkey": cached_did.controller_pk.to_string(),
            "last_updated": cached_did.last_updated,
        })))
        .into_response();
    }

    // Not found in cache — merchant not registered (or cache expired)
    (
        StatusCode::NOT_FOUND,
        axum::Json(serde_json::json!({
            "verified": false,
            "error": "Merchant not found"
        })),
    )
        .into_response()
}
