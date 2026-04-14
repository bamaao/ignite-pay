use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use tracing::info;

use crate::did::resolver::compute_did_hash;
use crate::error::RegistryError;
use crate::state::RegistryState;

/// Request body for updating the platform VC hash.
#[derive(Debug, Deserialize)]
pub struct UpdateVcRequest {
    pub merchant_did: String,
    /// Hex-encoded new VC hash (32 bytes)
    pub new_vc_hash: String,
    /// Base64-encoded platform signature over "update-vc:{merchant_did}:{new_vc_hash}"
    pub platform_signature: String,
}

/// `POST /v1/merchants/update-vc` — Update the platform VC hash for a merchant.
pub async fn update_vc(
    State(state): State<RegistryState>,
    axum::Json(req): axum::Json<UpdateVcRequest>,
) -> impl IntoResponse {
    if !req.merchant_did.starts_with("did:ignite:") {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": "Invalid DID format" })),
        )
            .into_response();
    }

    // Parse new VC hash
    let new_vc_hash = match hex_to_bytes32(&req.new_vc_hash) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": format!("Invalid new_vc_hash: {}", e) })),
            )
                .into_response();
        }
    };

    // TODO: Verify platform_signature using config.auth.platform_public_key
    // For now, accept the request (platform auth will be enforced in production)

    info!("Updating VC hash for merchant {}", req.merchant_did);

    let did_hash = compute_did_hash(&req.merchant_did);

    // Look up current leaf
    let (leaf_index, old_leaf) = match state.get_cached_merchant(&did_hash) {
        Some(data) => data,
        None => {
            match state
                .indexer
                .find_merchant_leaf(&state.compression.tree_address, &req.merchant_did)
                .await
            {
                Ok(Some(data)) => data,
                Ok(None) => {
                    return (
                        StatusCode::NOT_FOUND,
                        axum::Json(serde_json::json!({ "error": "Merchant not found" })),
                    )
                        .into_response();
                }
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        axum::Json(serde_json::json!({ "error": format!("Indexer error: {}", e) })),
                    )
                        .into_response();
                }
            }
        }
    };

    // Get Merkle proof
    let proof = match state
        .indexer
        .get_merkle_proof(&state.compression.tree_address, leaf_index)
        .await
    {
        Ok(p) => p.proof,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": format!("Failed to get proof: {}", e) })),
            )
                .into_response();
        }
    };

    // Build new leaf
    let slot = state
        .compression
        .rpc_client
        .get_slot()
        .map_err(|e| RegistryError::OnChain(e.to_string()))
        .unwrap_or(old_leaf.slot_updated + 1);

    let mut new_leaf = old_leaf.clone();
    new_leaf.platform_vc_hash = new_vc_hash;
    new_leaf.slot_updated = slot;

    // Submit update on-chain
    match state
        .compression
        .update_merchant(&state.payer, &old_leaf, &new_leaf, leaf_index, &proof)
        .await
    {
        Ok(sig) => {
            state.cache_merchant(&did_hash, leaf_index, &new_leaf);
            info!("VC updated for {}: sig={}", req.merchant_did, sig);
            (StatusCode::OK, axum::Json(serde_json::json!({
                "signature": sig.to_string(),
                "slot": slot,
            })))
            .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to update VC on-chain: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": format!("On-chain error: {}", e) })),
            )
                .into_response()
        }
    }
}

fn hex_to_bytes32(hex_str: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(hex_str).map_err(|e| format!("Invalid hex: {}", e))?;
    if bytes.len() != 32 {
        return Err(format!("Expected 32 bytes, got {}", bytes.len()));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}
