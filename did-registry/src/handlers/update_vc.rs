// Copyright (c) 2026 zouyc zouyccq@gmail.com.
// All rights reserved.
//
// Licensed under the Business Source License 1.1 (BSL 1.1).
// You may not use this file except in compliance with the License.
//
// Change Date: 2031-01-01
// On the Change Date, or the fourth anniversary of the first publicly available
// distribution of the code under the BSL, whichever comes first, the code
// automatically becomes available under the Apache License 2.0.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use base64::Engine;
use serde::Deserialize;
use tracing::info;

use crate::did::resolver::compute_did_hash;
use crate::handlers::nonce::verify_and_consume_nonce;
use crate::state::RegistryState;
use ignite_pay_solana::types::OnchainMode;

#[cfg(feature = "zk-compression")]
use light_client::rpc::Rpc;
#[cfg(feature = "zk-compression")]
use light_client::indexer::Indexer;
#[cfg(feature = "zk-compression")]
use light_sdk::instruction::account_meta::CompressedAccountMeta;

/// Request body for updating the platform VC hash.
#[derive(Debug, Deserialize)]
pub struct UpdateVcRequest {
    pub merchant_did: String,
    /// Hex-encoded new VC hash (32 bytes)
    pub new_vc_hash: String,
    /// Base64-encoded platform signature over "update-vc:{merchant_did}:{new_vc_hash}:{nonce}"
    pub platform_signature: String,
    /// Server-issued nonce to prevent replay. Obtain from GET /v1/auth/nonce.
    pub nonce: String,
    /// Borsh-serialized CompressedAccountMeta for the current account.
    #[cfg(feature = "zk-compression")]
    #[serde(default)]
    pub account_meta_b64: Option<String>,
    /// On-chain submission mode. Defaults to `sponsored` (backward compatible).
    #[serde(default)]
    pub mode: OnchainMode,
}

// ─── PDA version (default) ──────────────────────────────────────────

/// `POST /v1/merchants/update-vc` — Update the platform VC hash for a merchant (PDA DID).
#[cfg(not(feature = "zk-compression"))]
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

    // Verify nonce was issued by this server and consume it (prevents replay)
    if !verify_and_consume_nonce(&state, &req.nonce) {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Invalid or expired nonce" })),
        )
            .into_response();
    }

    // Verify platform_signature using config.auth.platform_public_key
    let message = format!("update-vc:{}:{}:{}", req.merchant_did, req.new_vc_hash, req.nonce);
    if !verify_platform_signature(&state.config.auth.platform_public_key, &message, &req.platform_signature) {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Invalid platform signature" })),
        )
            .into_response();
    }

    info!("Updating VC hash for merchant {} (PDA)", req.merchant_did);

    let did_hash = compute_did_hash(&req.merchant_did);

    // Look up current DID account from cache
    let current_did = match state.get_cached_merchant(&did_hash) {
        Some(did) => did,
        None => {
            return (
                StatusCode::NOT_FOUND,
                axum::Json(serde_json::json!({ "error": "Merchant not found" })),
            )
                .into_response();
        }
    };

    // Generate platform signature over (credential_subject_pk || new_vc_hash)
    let platform_signature = state.sign_vc_binding(&current_did.controller_pk, &new_vc_hash);
    let credential_subject_pk = current_did.controller_pk;

    match req.mode {
        OnchainMode::Sponsored => {
            match state
                .did_service
                .update_did_with_vc(
                    &state.payer,
                    new_vc_hash,
                    current_did.nonce,
                    platform_signature,
                    &credential_subject_pk,
                )
                .await
            {
                Ok(sig) => {
                    // Update cached merchant
                    let mut updated_did = current_did.clone();
                    updated_did.vc_hash = new_vc_hash;
                    updated_did.last_updated = chrono::Utc::now().timestamp();
                    updated_did.nonce = current_did.nonce + 1;
                    state.cache_merchant(&did_hash, &updated_did);

                    // Record fee
                    let store = crate::storage::sled_store::MerchantStore::new((*state.db).clone());
                    if let Err(e) = store.record_fee(
                        &did_hash,
                        "update_vc",
                        state.config.fees.update_vc_fee_lamports,
                        "sponsored",
                        &req.merchant_did,
                    ) {
                        tracing::warn!("Failed to record fee: {}", e);
                    }

                    info!("VC updated for {}: sig={}", req.merchant_did, sig);
                    (StatusCode::OK, axum::Json(serde_json::json!({
                        "signature": sig.to_string(),
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
        OnchainMode::SelfOnchain => {
            // Use the current controller as signer
            let signer_pubkey = current_did.controller_pk;

            let tx = match state
                .did_service
                .prepare_update_did_with_vc(
                    &signer_pubkey,
                    new_vc_hash,
                    current_did.nonce,
                    platform_signature,
                    &credential_subject_pk,
                )
                .await
            {
                Ok(tx) => tx,
                Err(e) => {
                    tracing::error!("Failed to prepare unsigned transaction: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        axum::Json(serde_json::json!({ "error": format!("Prepare error: {}", e) })),
                    )
                        .into_response();
                }
            };

            let tx_bytes = match bincode::serialize(&tx) {
                Ok(b) => b,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        axum::Json(serde_json::json!({ "error": format!("Serialization error: {}", e) })),
                    )
                        .into_response();
                }
            };

            let tx_b64 = base64::engine::general_purpose::STANDARD.encode(&tx_bytes);

            info!("Prepared unsigned update-vc transaction for merchant {}", req.merchant_did);
            (StatusCode::OK, axum::Json(serde_json::json!({
                "transaction": tx_b64,
                "message": "sign and broadcast within 90 seconds; blockhash expires",
            })))
            .into_response()
        }
    }
}

// ─── ZK Compression version (optional) ──────────────────────────────

/// `POST /v1/merchants/update-vc` — Update the platform VC hash for a merchant (compressed DID).
#[cfg(feature = "zk-compression")]
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

    // Verify nonce was issued by this server and consume it (prevents replay)
    if !verify_and_consume_nonce(&state, &req.nonce) {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Invalid or expired nonce" })),
        )
            .into_response();
    }

    // Verify platform_signature using config.auth.platform_public_key
    let message = format!("update-vc:{}:{}:{}", req.merchant_did, req.new_vc_hash, req.nonce);
    if !verify_platform_signature(&state.config.auth.platform_public_key, &message, &req.platform_signature) {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Invalid platform signature" })),
        )
            .into_response();
    }

    info!("Updating VC hash for merchant {}", req.merchant_did);

    let did_hash = compute_did_hash(&req.merchant_did);

    // Look up current DID account from cache
    let current_did = match state.get_cached_merchant(&did_hash) {
        Some(did) => did,
        None => {
            return (
                StatusCode::NOT_FOUND,
                axum::Json(serde_json::json!({ "error": "Merchant not found" })),
            )
                .into_response();
        }
    };

    // Parse account_meta from base64 if provided
    let account_meta = match req.account_meta_b64 {
        Some(ref b64) => {
            let bytes = match base64::engine::general_purpose::STANDARD_NO_PAD.decode(b64) {
                Ok(b) => b,
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({ "error": format!("Invalid account_meta: {}", e) })),
                    )
                        .into_response();
                }
            };
            match <CompressedAccountMeta as borsh::BorshDeserialize>::deserialize(&mut bytes.as_slice()) {
                Ok(meta) => meta,
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({ "error": format!("Failed to deserialize account_meta: {}", e) })),
                    )
                        .into_response();
                }
            }
        }
        None => CompressedAccountMeta::default(),
    };

    // Build updated DID account
    let mut updated_did = current_did.clone();
    updated_did.vc_hash = new_vc_hash;
    updated_did.last_updated = chrono::Utc::now().timestamp();
    updated_did.nonce = current_did.nonce + 1;

    // Get validity proof from Light RPC
    let light_rpc = state.light_rpc.lock().await;
    let indexer = match light_rpc.indexer() {
        Ok(i) => i,
        Err(e) => {
            tracing::error!("Indexer error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": format!("Indexer error: {}", e) })),
            )
                .into_response();
        }
    };
    let proof_result = match indexer
        .get_validity_proof(
            vec![light_client::indexer::Hash::from(current_did.vc_hash)],
            vec![],
            None,
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to get validity proof: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": format!("Proof error: {}", e) })),
            )
                .into_response();
        }
    };

    let proof_context = proof_result.value;
    let mut packed_accounts = light_account::PackedAccounts::default();
    let _packed_tree_infos = proof_context.pack_tree_infos(&mut packed_accounts);
    let remaining_accounts: Vec<solana_sdk::instruction::AccountMeta> =
        packed_accounts.to_account_metas().0;

    let proof_bytes = match borsh::to_vec(&proof_context.proof) {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": format!("Serialization error: {}", e) })),
            )
                .into_response();
        }
    };

    // Generate platform signature over (credential_subject_pk || new_vc_hash)
    let platform_signature = state.sign_vc_binding(&current_did.controller_pk, &new_vc_hash);
    let platform_config_address = state.platform_config_address();
    let credential_subject_pk = current_did.controller_pk;

    match req.mode {
        OnchainMode::Sponsored => {
            match state
                .did_service
                .update_did_with_vc(
                    &state.payer,
                    &proof_bytes,
                    &current_did,
                    &account_meta,
                    new_vc_hash,
                    current_did.nonce,
                    platform_signature,
                    &credential_subject_pk,
                    &platform_config_address,
                    &remaining_accounts,
                )
                .await
            {
                Ok(sig) => {
                    state.cache_merchant(&did_hash, &updated_did);

                    // Record fee
                    let store = crate::storage::sled_store::MerchantStore::new((*state.db).clone());
                    if let Err(e) = store.record_fee(
                        &did_hash,
                        "update_vc",
                        state.config.fees.update_vc_fee_lamports,
                        "sponsored",
                        &req.merchant_did,
                    ) {
                        tracing::warn!("Failed to record fee: {}", e);
                    }

                    info!("VC updated for {}: sig={}", req.merchant_did, sig);
                    (StatusCode::OK, axum::Json(serde_json::json!({
                        "signature": sig.to_string(),
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
        OnchainMode::SelfOnchain => {
            // Use the current controller as signer
            let signer_pubkey = current_did.controller_pk;

            let tx = match state
                .did_service
                .prepare_update_did_with_vc(
                    &signer_pubkey,
                    &proof_bytes,
                    &current_did,
                    &account_meta,
                    new_vc_hash,
                    current_did.nonce,
                    platform_signature,
                    &credential_subject_pk,
                    &platform_config_address,
                    &remaining_accounts,
                )
                .await
            {
                Ok(tx) => tx,
                Err(e) => {
                    tracing::error!("Failed to prepare unsigned transaction: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        axum::Json(serde_json::json!({ "error": format!("Prepare error: {}", e) })),
                    )
                        .into_response();
                }
            };

            let tx_bytes = match bincode::serialize(&tx) {
                Ok(b) => b,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        axum::Json(serde_json::json!({ "error": format!("Serialization error: {}", e) })),
                    )
                        .into_response();
                }
            };

            let tx_b64 = base64::engine::general_purpose::STANDARD.encode(&tx_bytes);

            info!("Prepared unsigned update-vc transaction for merchant {}", req.merchant_did);
            (StatusCode::OK, axum::Json(serde_json::json!({
                "transaction": tx_b64,
                "message": "sign and broadcast within 90 seconds; blockhash expires",
            })))
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
