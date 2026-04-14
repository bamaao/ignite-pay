use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use tracing::info;

use crate::did::resolver::compute_did_hash;
use crate::state::RegistryState;

/// `GET /v1/merchants/verify/{did}` — Verify a merchant on-chain with Merkle proof.
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

    // Try local cache first
    if let Some((leaf_index, leaf)) = state.get_cached_merchant(&did_hash) {
        // Get Merkle proof from indexer
        match state.indexer.get_merkle_proof(&state.compression.tree_address, leaf_index).await {
            Ok(proof) => {
                let verified = state.compression.verify_proof_locally(
                    &proof.leaf_hash,
                    &proof.proof,
                    &proof.root,
                );

                return (StatusCode::OK, axum::Json(serde_json::json!({
                    "verified": verified,
                    "leaf": {
                        "merchant_did_hash": hex::encode(&leaf.merchant_did_hash),
                        "active_pubkey": leaf.active_pubkey.to_string(),
                        "status": leaf.status,
                        "slot_updated": leaf.slot_updated,
                    },
                    "proof": {
                        "leaf_index": proof.leaf_index,
                        "root": hex::encode(&proof.root),
                    }
                }))).into_response();
            }
            Err(e) => {
                tracing::warn!("Failed to get Merkle proof: {}", e);
                // Fall through to return cached data without proof
            }
        }
    }

    // Try on-chain lookup via indexer
    match state.indexer.find_merchant_leaf(&state.compression.tree_address, &did).await {
        Ok(Some((leaf_index, leaf))) => {
            state.cache_merchant(&did_hash, leaf_index, &leaf);
            (StatusCode::OK, axum::Json(serde_json::json!({
                "verified": true,
                "leaf": {
                    "merchant_did_hash": hex::encode(&leaf.merchant_did_hash),
                    "active_pubkey": leaf.active_pubkey.to_string(),
                    "status": leaf.status,
                    "slot_updated": leaf.slot_updated,
                }
            })))
            .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({
                "verified": false,
                "error": "Merchant not found on-chain"
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({ "error": format!("Indexer error: {}", e) })),
        )
            .into_response(),
    }
}
