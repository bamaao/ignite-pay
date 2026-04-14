use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use tracing::info;

use crate::did::resolver::compute_did_hash;
use crate::error::RegistryError;
use crate::state::RegistryState;
use ignite_pay_solana::types::MerchantLeaf;
use solana_sdk::signature::Signature;

/// Request body for merchant registration.
#[derive(Debug, Deserialize)]
pub struct RegisterMerchantRequest {
    pub merchant_did: String,
    pub active_pubkey: String,
    pub platform_vc_hash: String, // hex-encoded 32 bytes
    #[serde(default = "default_status")]
    pub status: u8,
}

fn default_status() -> u8 {
    0 // active
}

/// `POST /v1/merchants/register` — Register a merchant on-chain.
pub async fn register_merchant(
    State(state): State<RegistryState>,
    axum::Json(req): axum::Json<RegisterMerchantRequest>,
) -> impl IntoResponse {
    // Validate DID format
    if !req.merchant_did.starts_with("did:ignite:") {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": "Invalid DID format, expected did:ignite:..." })),
        )
            .into_response();
    }

    // Parse active_pubkey
    let active_pubkey = match req.active_pubkey.parse::<solana_sdk::pubkey::Pubkey>() {
        Ok(pk) => pk,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": format!("Invalid active_pubkey: {}", e) })),
            )
                .into_response();
        }
    };

    // Parse platform_vc_hash (hex)
    let vc_hash_bytes = match hex_to_bytes32(&req.platform_vc_hash) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": format!("Invalid platform_vc_hash: {}", e) })),
            )
                .into_response();
        }
    };

    // Build MerchantLeaf
    let did_hash = compute_did_hash(&req.merchant_did);
    let slot = state.compression.rpc_client
        .get_slot()
        .map_err(|e| RegistryError::OnChain(e.to_string()))
        .unwrap_or(0);

    let leaf = MerchantLeaf {
        merchant_did_hash: did_hash,
        active_pubkey,
        platform_vc_hash: vc_hash_bytes,
        status: req.status,
        slot_updated: slot,
    };

    info!("Registering merchant {} on-chain", req.merchant_did);

    // Submit to on-chain Merkle tree
    match state.compression.add_merchant(&state.payer, &leaf).await {
        Ok(sig) => {
            info!("Merchant registered: sig={}", sig);
            (StatusCode::OK, axum::Json(serde_json::json!({
                "signature": sig.to_string(),
                "slot": slot,
            })))
            .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to register merchant on-chain: {}", e);
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
