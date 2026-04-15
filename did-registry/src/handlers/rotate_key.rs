use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use tracing::info;

use crate::did::resolver::{compute_did_hash, verify_did_signature};
use crate::error::RegistryError;
use crate::handlers::nonce::verify_and_consume_nonce;
use crate::state::RegistryState;

/// Request body for key rotation.
#[derive(Debug, Deserialize)]
pub struct RotateKeyRequest {
    pub merchant_did: String,
    pub new_active_pubkey: String,
    /// Base64-encoded Ed25519 signature from the DID key
    pub did_signature: String,
    /// Server-issued nonce to prevent replay. Obtain from GET /v1/auth/nonce.
    pub nonce: String,
}

/// `POST /v1/merchants/rotate-key` — Rotate a merchant's active Solana pubkey.
pub async fn rotate_key(
    State(state): State<RegistryState>,
    axum::Json(req): axum::Json<RotateKeyRequest>,
) -> impl IntoResponse {
    if !req.merchant_did.starts_with("did:ignite:") {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": "Invalid DID format" })),
        )
            .into_response();
    }

    // Parse new pubkey
    let new_pubkey = match req.new_active_pubkey.parse::<solana_sdk::pubkey::Pubkey>() {
        Ok(pk) => pk,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": format!("Invalid new_active_pubkey: {}", e) })),
            )
                .into_response();
        }
    };

    // Verify nonce was issued by this server and consume it (prevents replay)
    if !verify_and_consume_nonce(&state, &req.nonce) {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Invalid or expired nonce" })),
        )
            .into_response();
    }

    // Verify DID signature over the new pubkey + nonce
    let message = format!("rotate-key:{}:{}:{}", req.merchant_did, req.new_active_pubkey, req.nonce);
    if !verify_did_signature(&req.merchant_did, &message, &req.did_signature) {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Invalid DID signature" })),
        )
            .into_response();
    }

    info!("Rotating key for merchant {}", req.merchant_did);

    let did_hash = compute_did_hash(&req.merchant_did);

    // Look up current leaf
    let (leaf_index, old_leaf) = match state.get_cached_merchant(&did_hash) {
        Some(data) => data,
        None => {
            // Try on-chain lookup
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

    // Build new leaf with updated pubkey
    let slot = state
        .compression
        .rpc_client
        .get_slot()
        .map_err(|e| RegistryError::OnChain(e.to_string()))
        .unwrap_or(old_leaf.slot_updated + 1);

    let mut new_leaf = old_leaf.clone();
    new_leaf.active_pubkey = new_pubkey;
    new_leaf.slot_updated = slot;

    // Submit update on-chain
    match state
        .compression
        .update_merchant(&state.payer, &old_leaf, &new_leaf, leaf_index, &proof)
        .await
    {
        Ok(sig) => {
            state.cache_merchant(&did_hash, leaf_index, &new_leaf);
            info!("Key rotated for {}: sig={}", req.merchant_did, sig);
            (StatusCode::OK, axum::Json(serde_json::json!({
                "signature": sig.to_string(),
                "slot": slot,
            })))
            .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to rotate key on-chain: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": format!("On-chain error: {}", e) })),
            )
                .into_response()
        }
    }
}
