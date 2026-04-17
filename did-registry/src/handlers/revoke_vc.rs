use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use base64::Engine;
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use tracing::info;

use crate::state::RegistryState;

/// Request body for revoking a VC.
#[derive(Debug, Deserialize)]
pub struct RevokeVcRequest {
    /// Hex-encoded 32-byte VC hash to revoke.
    pub vc_hash: String,
    /// The credential subject's public key (base58).
    pub credential_subject_pk: String,
    /// Revocation reason (0=unspecified, 1=violation, 2=expired, etc.).
    #[serde(default)]
    pub reason: u8,
    /// Platform authority signature over "revoke:{vc_hash}:{nonce}".
    pub platform_signature: String,
    /// Server-issued nonce.
    pub nonce: String,
}

/// `POST /v1/vc/revoke` — Revoke a VC by creating an on-chain RevokedVc PDA.
///
/// Only the platform authority can revoke. Creates a PDA with seeds
/// `[b"revoked-vc", vc_hash]` on-chain. Verifiers check PDA existence
/// to determine if a VC has been revoked.
pub async fn revoke_vc(
    State(state): State<RegistryState>,
    axum::Json(req): axum::Json<RevokeVcRequest>,
) -> impl IntoResponse {
    // Parse vc_hash
    let vc_hash_bytes = match hex_to_bytes32(&req.vc_hash) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": format!("Invalid vc_hash: {}", e) })),
            )
                .into_response();
        }
    };

    // Parse credential_subject_pk
    let credential_subject_pk = match req.credential_subject_pk.parse::<Pubkey>() {
        Ok(pk) => pk,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": format!("Invalid credential_subject_pk: {}", e) })),
            )
                .into_response();
        }
    };

    // Verify nonce
    if !crate::handlers::nonce::verify_and_consume_nonce(&state, &req.nonce) {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Invalid or expired nonce" })),
        )
            .into_response();
    }

    // Verify platform signature: "revoke:{vc_hash}:{nonce}"
    let message = format!("revoke:{}:{}", req.vc_hash, req.nonce);
    if !verify_platform_signature(&state, &message, &req.platform_signature) {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Invalid platform signature" })),
        )
            .into_response();
    }

    info!("Revoking VC {} for subject {}", req.vc_hash, req.credential_subject_pk);

    let platform_config_address = state.platform_config_address();

    match state
        .did_service
        .revoke_vc(
            &state.payer,
            vc_hash_bytes,
            &credential_subject_pk,
            req.reason,
            &platform_config_address,
        )
        .await
    {
        Ok(sig) => {
            // Cache revocation locally in sled
            let store = crate::storage::sled_store::MerchantStore::new((*state.db).clone());
            if let Err(e) = store.mark_vc_revoked(&req.vc_hash, &req.credential_subject_pk, req.reason) {
                tracing::warn!("Failed to cache revocation: {}", e);
            }

            info!("VC revoked on-chain: vc_hash={} sig={}", req.vc_hash, sig);
            (StatusCode::OK, axum::Json(serde_json::json!({
                "signature": sig.to_string(),
                "revoked_vc_pda": state.revoked_vc_address(&vc_hash_bytes).to_string(),
            })))
            .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to revoke VC on-chain: {}", e);
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

/// Verify an Ed25519 signature against the platform public key (base64-encoded).
fn verify_platform_signature(
    state: &RegistryState,
    message: &str,
    signature_b64: &str,
) -> bool {
    use ed25519_dalek::Verifier;
    let verifying_key = state.platform_signing_key.verifying_key();

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

    verifying_key.verify(message.as_bytes(), &sig).is_ok()
}
