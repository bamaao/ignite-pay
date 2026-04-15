use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use base64::Engine;
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

    // Verify platform_signature using config.auth.platform_public_key
    let message = format!("update-vc:{}:{}", req.merchant_did, req.new_vc_hash);
    if !verify_platform_signature(&state.config.auth.platform_public_key, &message, &req.platform_signature) {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Invalid platform signature" })),
        )
            .into_response();
    }

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

/// Verify an Ed25519 signature against a platform public key (base64-encoded).
fn verify_platform_signature(
    pubkey_b64: &str,
    message: &str,
    signature_b64: &str,
) -> bool {
    let pk_bytes = match base64::engine::general_purpose::STANDARD_NO_PAD.decode(pubkey_b64) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };
    if pk_bytes.len() != 32 {
        return false;
    }
    let mut pk_arr = [0u8; 32];
    pk_arr.copy_from_slice(&pk_bytes);

    let verifying_key = match ed25519_dalek::VerifyingKey::from_bytes(&pk_arr) {
        Ok(key) => key,
        Err(_) => return false,
    };

    let sig_bytes = match base64::engine::general_purpose::STANDARD_NO_PAD.decode(signature_b64) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };
    if sig_bytes.len() != 64 {
        return false;
    }
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);
    let sig = match ed25519_dalek::Signature::try_from(sig_arr.as_slice()) {
        Ok(s) => s,
        Err(_) => return false,
    };

    use ed25519_dalek::Verifier;
    verifying_key.verify(message.as_bytes(), &sig).is_ok()
}
