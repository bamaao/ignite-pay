use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use base64::Engine;
use light_client::indexer::Indexer;
use light_client::rpc::Rpc;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

use crate::state::RegistryState;

#[derive(Debug, Deserialize)]
pub struct ProofRequest {
    /// Merchant's active/controller public key (base58).
    pub pubkey: String,
    /// Which operation the proof is for: "register", "update_vc", or "rotate_key".
    #[serde(default = "default_operation")]
    pub operation: String,
    /// Hex-encoded 32-byte hash of the existing compressed account.
    /// Required for "update_vc" and "rotate_key" operations.
    /// Not needed for "register".
    pub account_hash: Option<String>,
}

fn default_operation() -> String {
    "register".to_string()
}

#[derive(Debug, Serialize)]
pub struct AccountMetaJson {
    pub pubkey: String,
    pub is_signer: bool,
    pub is_writable: bool,
}

#[derive(Debug, Serialize)]
pub struct ProofResponse {
    /// Borsh-serialized ZK validity proof (base64).
    pub proof: String,
    /// Compressed address derived from the merchant pubkey (base58).
    pub compressed_address: String,
    /// Address seed used for derivation (base58).
    pub address_seed: String,
    /// Address Merkle tree pubkey (base58).
    pub address_merkle_tree: String,
    /// Packed address tree info: Borsh-serialized (base64).
    pub address_tree_info: String,
    /// Output state tree index.
    pub output_state_tree_index: u8,
    /// Remaining accounts for the Light CPI (ZK compression accounts).
    pub remaining_accounts: Vec<AccountMetaJson>,
    /// DID program ID (base58).
    pub program_id: String,
    /// Platform config PDA address (base58). Must be included in the accounts
    /// list for `initialize_did` and `update_did_with_vc` instructions.
    pub platform_config_address: String,
}

/// `POST /v1/proof` — Public endpoint for ZK Compression validity proofs.
///
/// Merchants call this to obtain the proof data needed to construct and sign
/// their own on-chain transactions locally, without the platform building or
/// seeing the transaction.
///
/// The merchant then:
/// 1. Builds the Anchor instruction using the program ID, discriminator,
///    accounts (signer + remaining_accounts), and instruction data
///    (proof + address_tree_info + params).
/// 2. Constructs a `Transaction` with a recent blockhash.
/// 3. Signs with their own keypair.
/// 4. Broadcasts to the Solana RPC.
pub async fn get_proof(
    State(state): State<RegistryState>,
    axum::Json(req): axum::Json<ProofRequest>,
) -> impl IntoResponse {
    let active_pubkey = match req.pubkey.parse::<Pubkey>() {
        Ok(pk) => pk,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": format!("Invalid pubkey: {}", e) })),
            )
                .into_response();
        }
    };

    if !matches!(req.operation.as_str(), "register" | "update_vc" | "rotate_key") {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "error": "operation must be one of: register, update_vc, rotate_key"
            })),
        )
            .into_response();
    }

    // For update_vc/rotate_key, account_hash is required
    if req.operation.as_str() != "register" && req.account_hash.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "error": "account_hash is required for update_vc and rotate_key operations"
            })),
        )
            .into_response();
    }

    // Parse account_hash if provided
    let account_hash_bytes: Option<[u8; 32]> = match req.account_hash {
        Some(ref hex_str) => {
            match hex_to_bytes32(hex_str) {
                Ok(h) => Some(h),
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({
                            "error": format!("Invalid account_hash: {}", e)
                        })),
                    )
                        .into_response();
                }
            }
        }
        None => None,
    };

    // Get address tree and derive compressed address
    let light_rpc = state.light_rpc.lock().await;
    let address_tree = light_rpc.get_address_tree_v1();
    let (address, seed) = state
        .did_service
        .derive_compressed_address(&active_pubkey, &address_tree.tree);

    // Get validity proof
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

    let proof_result = match req.operation.as_str() {
        "register" => {
            // New address proof: no existing accounts, one new address
            indexer
                .get_validity_proof(
                    vec![],
                    vec![light_client::indexer::AddressWithTree {
                        address: light_client::indexer::Address::from(address),
                        tree: address_tree.tree,
                    }],
                    None,
                )
                .await
        }
        _ => {
            // Existing account proof: provide the account hash
            let hash = account_hash_bytes.unwrap();
            indexer
                .get_validity_proof(
                    vec![light_client::indexer::Hash::from(hash)],
                    vec![],
                    None,
                )
                .await
        }
    };

    let proof_result = match proof_result {
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

    let address_tree_info_bytes = match borsh::to_vec(&address_tree_info) {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": format!("Serialization error: {}", e) })),
            )
                .into_response();
        }
    };

    let response = ProofResponse {
        proof: base64::engine::general_purpose::STANDARD.encode(&proof_bytes),
        compressed_address: bs58::encode(address).into_string(),
        address_seed: bs58::encode(&seed.0).into_string(),
        address_merkle_tree: address_tree.tree.to_string(),
        address_tree_info: base64::engine::general_purpose::STANDARD.encode(&address_tree_info_bytes),
        output_state_tree_index,
        remaining_accounts: remaining_accounts
            .into_iter()
            .map(|meta| AccountMetaJson {
                pubkey: meta.pubkey.to_string(),
                is_signer: meta.is_signer,
                is_writable: meta.is_writable,
            })
            .collect(),
        program_id: state.did_program_id().to_string(),
        platform_config_address: state.platform_config_address().to_string(),
    };

    (StatusCode::OK, axum::Json(serde_json::to_value(response).unwrap())).into_response()
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
