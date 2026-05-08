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
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use tracing::info;

use crate::did::resolver::compute_did_hash;
use crate::handlers::nonce::verify_and_consume_nonce;
use crate::state::RegistryState;
use ignite_pay_core::verify_did_signature;
use ignite_pay_solana::types::MerchantDidAccount;

/// Request body for confirming a SelfOnchain registration.
#[derive(Debug, Deserialize)]
pub struct ConfirmRegisterRequest {
    pub merchant_did: String,
    /// Solana transaction signature from the broadcast.
    pub tx_signature: String,
    /// Active pubkey that was used as the on-chain signer.
    pub active_pubkey: String,
    /// Platform VC hash that was anchored on-chain.
    pub platform_vc_hash: String,
    /// DID signature over "confirm:{did}:{tx_signature}:{nonce}"
    pub did_signature: String,
    /// Server-issued nonce.
    pub nonce: String,
}

/// `POST /v1/merchants/confirm` — Confirm a SelfOnchain registration after successful broadcast.
///
/// After the merchant signs and broadcasts the unsigned transaction returned by
/// `POST /v1/merchants/register` (mode=self_onchain), they call this endpoint to
/// notify the platform. The platform verifies the transaction is on-chain and
/// caches the merchant data locally so subsequent operations (verify, status,
/// update-vc, rotate-key) can work.
pub async fn confirm_register(
    State(state): State<RegistryState>,
    axum::Json(req): axum::Json<ConfirmRegisterRequest>,
) -> impl IntoResponse {
    if !req.merchant_did.starts_with("did:ignite:") {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": "Invalid DID format" })),
        )
            .into_response();
    }

    // Verify and consume nonce
    if !verify_and_consume_nonce(&state, &req.nonce) {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Invalid or expired nonce" })),
        )
            .into_response();
    }

    // Verify DID signature proving ownership
    let message = format!(
        "confirm:{}:{}:{}",
        req.merchant_did, req.tx_signature, req.nonce
    );
    if !verify_did_signature(&req.merchant_did, &message, &req.did_signature) {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Invalid DID signature" })),
        )
            .into_response();
    }

    // Parse active_pubkey
    let active_pubkey = match req.active_pubkey.parse::<Pubkey>() {
        Ok(pk) => pk,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": format!("Invalid active_pubkey: {}", e) })),
            )
                .into_response();
        }
    };

    // Parse tx_signature
    let tx_sig = match req.tx_signature.parse::<Signature>() {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": format!("Invalid tx_signature: {}", e) })),
            )
                .into_response();
        }
    };

    // Parse vc_hash
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

    let did_hash = compute_did_hash(&req.merchant_did);

    // Idempotent: already cached
    if state.get_cached_merchant(&did_hash).is_some() {
        return (StatusCode::OK, axum::Json(serde_json::json!({
            "status": "already_confirmed",
        })))
        .into_response();
    }

    // Verify transaction exists on-chain
    match state
        .did_service
        .rpc_client
        .get_signature_statuses(&[tx_sig])
    {
        Ok(statuses) => {
            let confirmed = statuses
                .value
                .first()
                .and_then(|opt| opt.as_ref())
                .is_some_and(|s| s.confirmation_status == Some(solana_transaction_status::TransactionConfirmationStatus::Finalized)
                    || s.confirmation_status == Some(solana_transaction_status::TransactionConfirmationStatus::Confirmed));

            if !confirmed {
                return (
                    StatusCode::NOT_FOUND,
                    axum::Json(serde_json::json!({
                        "error": "Transaction not confirmed on-chain"
                    })),
                )
                    .into_response();
            }
        }
        Err(e) => {
            tracing::warn!("Failed to check tx status for {}: {}", tx_sig, e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({
                    "error": format!("RPC error: {}", e)
                })),
            )
                .into_response();
        }
    }

    // Cache the merchant locally
    let did_account = MerchantDidAccount {
        original_pk: active_pubkey,
        controller_pk: active_pubkey,
        recovery_pk: Pubkey::default(),
        vc_hash: vc_hash_bytes,
        last_updated: chrono::Utc::now().timestamp(),
        nonce: 0,
    };
    state.cache_merchant(&did_hash, &did_account);

    info!(
        "Confirmed SelfOnchain registration for {}: tx={}",
        req.merchant_did, tx_sig
    );

    (StatusCode::OK, axum::Json(serde_json::json!({
        "status": "confirmed",
    })))
    .into_response()
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
