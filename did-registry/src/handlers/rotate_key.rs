use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use base64::Engine;
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use tracing::info;

#[cfg(feature = "zk-compression")]
use light_client::rpc::Rpc;
#[cfg(feature = "zk-compression")]
use light_client::indexer::Indexer;

use crate::did::resolver::compute_did_hash;
use crate::handlers::nonce::verify_and_consume_nonce;
use crate::state::RegistryState;
use ignite_pay_core::verify_did_signature;
use ignite_pay_solana::types::OnchainMode;

/// Request body for key rotation (maps to set_recovery_key in DID).
#[derive(Debug, Deserialize)]
pub struct RotateKeyRequest {
    pub merchant_did: String,
    pub new_active_pubkey: String,
    /// Base64-encoded Ed25519 signature from the DID key
    pub did_signature: String,
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

/// `POST /v1/merchants/rotate-key` — Rotate a merchant's active Solana pubkey.
/// PDA version: updates the controller_pk on the PDA DID account.
#[cfg(not(feature = "zk-compression"))]
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
    let new_pubkey = match req.new_active_pubkey.parse::<Pubkey>() {
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

    match req.mode {
        OnchainMode::Sponsored => {
            match state
                .did_service
                .set_recovery_key(
                    &state.payer,
                    &new_pubkey,
                    current_did.nonce,
                )
                .await
            {
                Ok(sig) => {
                    // Update cache with new controller_pk and incremented nonce
                    let mut updated_did = current_did.clone();
                    updated_did.controller_pk = new_pubkey;
                    updated_did.last_updated = chrono::Utc::now().timestamp();
                    updated_did.nonce = current_did.nonce + 1;
                    state.cache_merchant(&did_hash, &updated_did);

                    // Record fee
                    let store = crate::storage::sled_store::MerchantStore::new((*state.db).clone());
                    if let Err(e) = store.record_fee(
                        &did_hash,
                        "rotate_key",
                        state.config.fees.rotate_key_fee_lamports,
                        "sponsored",
                        &req.merchant_did,
                    ) {
                        tracing::warn!("Failed to record fee: {}", e);
                    }

                    info!("Key rotated for {}: sig={}", req.merchant_did, sig);
                    (StatusCode::OK, axum::Json(serde_json::json!({
                        "signature": sig.to_string(),
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
        OnchainMode::SelfOnchain => {
            let signer_pubkey = current_did.controller_pk;

            let tx = match state
                .did_service
                .prepare_set_recovery_key(
                    &signer_pubkey,
                    &new_pubkey,
                    current_did.nonce,
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

            // Update cache with new controller_pk and incremented nonce
            let mut updated_did = current_did.clone();
            updated_did.controller_pk = new_pubkey;
            updated_did.last_updated = chrono::Utc::now().timestamp();
            updated_did.nonce = current_did.nonce + 1;
            state.cache_merchant(&did_hash, &updated_did);

            info!("Prepared unsigned rotate-key transaction for merchant {}", req.merchant_did);
            (StatusCode::OK, axum::Json(serde_json::json!({
                "transaction": tx_b64,
                "message": "sign and broadcast within 90 seconds; blockhash expires",
            })))
            .into_response()
        }
    }
}

// ─── ZK Compression version (optional) ──────────────────────────────

#[cfg(feature = "zk-compression")]
use light_sdk::instruction::account_meta::CompressedAccountMeta;

#[cfg(feature = "zk-compression")]
/// `POST /v1/merchants/rotate-key` — Rotate a merchant's active Solana pubkey.
/// ZK Compression version: updates the controller_pk on the compressed DID account.
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
    let new_pubkey = match req.new_active_pubkey.parse::<Pubkey>() {
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

    // Parse account_meta from base64 if provided, otherwise use default
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
    updated_did.controller_pk = new_pubkey;
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

    match req.mode {
        OnchainMode::Sponsored => {
            match state
                .did_service
                .set_recovery_key(
                    &state.payer,
                    &proof_bytes,
                    &current_did,
                    &account_meta,
                    &new_pubkey,
                    current_did.nonce,
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
                        "rotate_key",
                        state.config.fees.rotate_key_fee_lamports,
                        "sponsored",
                        &req.merchant_did,
                    ) {
                        tracing::warn!("Failed to record fee: {}", e);
                    }

                    info!("Key rotated for {}: sig={}", req.merchant_did, sig);
                    (StatusCode::OK, axum::Json(serde_json::json!({
                        "signature": sig.to_string(),
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
        OnchainMode::SelfOnchain => {
            let signer_pubkey = current_did.controller_pk;

            let tx = match state
                .did_service
                .prepare_set_recovery_key(
                    &signer_pubkey,
                    &proof_bytes,
                    &current_did,
                    &account_meta,
                    &new_pubkey,
                    current_did.nonce,
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

            info!("Prepared unsigned rotate-key transaction for merchant {}", req.merchant_did);
            (StatusCode::OK, axum::Json(serde_json::json!({
                "transaction": tx_b64,
                "message": "sign and broadcast within 90 seconds; blockhash expires",
            })))
            .into_response()
        }
    }
}
