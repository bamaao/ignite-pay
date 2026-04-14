use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use tracing::info;

use crate::did::resolver::compute_did_hash;
use crate::state::RegistryState;

/// `GET /v1/merchants/status/{did}` — Check a merchant's on-chain status.
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
    let status_str = |s: u8| match s {
        0 => "active",
        1 => "suspended",
        2 => "revoked",
        _ => "unknown",
    };

    // Try cache first
    if let Some((_leaf_index, leaf)) = state.get_cached_merchant(&did_hash) {
        return (
            StatusCode::OK,
            axum::Json(serde_json::json!({
                "status": status_str(leaf.status),
                "slot_updated": leaf.slot_updated,
            })),
        )
            .into_response();
    }

    // Try on-chain
    match state
        .indexer
        .find_merchant_leaf(&state.compression.tree_address, &did)
        .await
    {
        Ok(Some((leaf_index, leaf))) => {
            state.cache_merchant(&did_hash, leaf_index, &leaf);
            (
                StatusCode::OK,
                axum::Json(serde_json::json!({
                    "status": status_str(leaf.status),
                    "slot_updated": leaf.slot_updated,
                })),
            )
                .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({ "error": "Merchant not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({ "error": format!("Indexer error: {}", e) })),
        )
            .into_response(),
    }
}
