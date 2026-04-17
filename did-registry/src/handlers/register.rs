use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use base64::Engine;
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use tracing::info;

use crate::did::resolver::compute_did_hash;
use crate::handlers::nonce::verify_and_consume_nonce;
use crate::state::RegistryState;
use ignite_pay_core::verify_did_signature;
use ignite_pay_solana::types::{MerchantDidAccount, OnchainMode};
use light_client::rpc::Rpc;
use light_client::indexer::Indexer;

/// Request body for merchant registration.
#[derive(Debug, Deserialize)]
pub struct RegisterMerchantRequest {
    pub merchant_did: String,
    pub active_pubkey: String,
    pub platform_vc_hash: String, // hex-encoded 32 bytes
    /// Base64-encoded Ed25519 signature from the DID key over
    /// "register:{merchant_did}:{active_pubkey}:{platform_vc_hash}:{nonce}"
    pub did_signature: String,
    /// Server-issued nonce to prevent replay. Obtain from GET /v1/auth/nonce.
    pub nonce: String,
    /// On-chain submission mode. Defaults to `sponsored` (backward compatible).
    #[serde(default)]
    pub mode: OnchainMode,
}

/// `POST /v1/merchants/register` — Register a merchant on-chain as a ZK compressed DID.
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

    // Verify nonce was issued by this server and consume it (prevents replay)
    if !verify_and_consume_nonce(&state, &req.nonce) {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Invalid or expired nonce" })),
        )
            .into_response();
    }

    // Verify DID signature proving ownership of the merchant DID key
    let message = format!(
        "register:{}:{}:{}:{}",
        req.merchant_did, req.active_pubkey, req.platform_vc_hash, req.nonce
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

    info!("Registering merchant {} as compressed DID", req.merchant_did);

    // Get validity proof from Light RPC for new address creation
    let light_rpc = state.light_rpc.lock().await;
    let address_tree = light_rpc.get_address_tree_v1();
    let (address, _seed) = state
        .did_service
        .derive_compressed_address(&active_pubkey, &address_tree.tree);

    // Get new address proof (proves address doesn't exist yet)
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
            vec![], // no existing accounts
            vec![light_client::indexer::AddressWithTree {
                address: light_client::indexer::Address::from(address),
                tree: address_tree.tree,
            }],
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

    // Pack accounts and tree infos
    let mut packed_accounts = light_account::PackedAccounts::default();
    let packed_tree_infos = proof_context.pack_tree_infos(&mut packed_accounts);
    let remaining_accounts: Vec<solana_sdk::instruction::AccountMeta> =
        packed_accounts.to_account_metas().0;

    // Serialize proof
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

    // Get address_tree_info and output_state_tree_index
    let address_tree_info = match packed_tree_infos.address_trees.first() {
        Some(info) => *info,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": "No address tree info in proof" })),
            )
                .into_response();
        }
    };
    let output_state_tree_index = packed_tree_infos
        .state_trees
        .as_ref()
        .map(|st| st.output_tree_index)
        .unwrap_or(0);

    // Submit to on-chain compressed account via DidService
    let did_hash = compute_did_hash(&req.merchant_did);

    match req.mode {
        OnchainMode::Sponsored => {
            match state
                .did_service
                .initialize_did(
                    &state.payer,
                    &proof_bytes,
                    &address_tree_info,
                    output_state_tree_index,
                    &remaining_accounts,
                )
                .await
            {
                Ok(sig) => {
                    // Cache the new merchant DID locally
                    let did_account = MerchantDidAccount {
                        original_pk: active_pubkey,
                        controller_pk: active_pubkey,
                        recovery_pk: Pubkey::default(),
                        vc_hash: vc_hash_bytes,
                        last_updated: chrono::Utc::now().timestamp(),
                        nonce: 0,
                    };
                    state.cache_merchant(&did_hash, &did_account);

                    // Record fee
                    let store = crate::storage::sled_store::MerchantStore::new((*state.db).clone());
                    if let Err(e) = store.record_fee(
                        &did_hash,
                        "register",
                        state.config.fees.register_fee_lamports,
                        "sponsored",
                        &req.merchant_did,
                    ) {
                        tracing::warn!("Failed to record fee: {}", e);
                    }

                    info!("Merchant registered as compressed DID: sig={}", sig);
                    (StatusCode::OK, axum::Json(serde_json::json!({
                        "signature": sig.to_string(),
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
        OnchainMode::SelfOnchain => {
            let tx = match state
                .did_service
                .prepare_initialize_did(
                    &active_pubkey,
                    &proof_bytes,
                    &address_tree_info,
                    output_state_tree_index,
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

            info!("Prepared unsigned register transaction for merchant {}", req.merchant_did);
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
