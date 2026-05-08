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
use axum::Json;
use ignite_pay_core::types::VerifiableCredential;
use ignite_pay_core::verify_did_signature;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::did::resolver::compute_did_hash;
use crate::state::RegistryState;

#[derive(Debug, Deserialize)]
pub struct IssueVcRequest {
    /// Merchant DID (did:ignite:z...)
    pub merchant_did: String,
    /// Human-readable merchant display name
    pub merchant_name: String,
    /// Optional business category (e.g. "retail")
    pub category: Option<String>,
    /// Credential validity in hours (default 8760 = 1 year)
    pub validity_hours: Option<u64>,
    /// Server nonce for replay protection
    pub nonce: String,
    /// DID signature over "issue_vc:{merchant_did}:{merchant_name}:{nonce}"
    pub did_signature: String,
}

/// `POST /v1/vc/issue` — Issue a W3C Verifiable Credential for a merchant.
///
/// For initial registration: verifies DID ownership via signature, then issues VC.
/// For updates: verifies DID ownership AND checks merchant is already registered.
pub async fn issue_vc(
    State(state): State<RegistryState>,
    Json(req): Json<IssueVcRequest>,
) -> impl IntoResponse {
    // 1. Validate merchant DID format
    if !req.merchant_did.starts_with("did:ignite:") {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "merchant_did must start with did:ignite:"})),
        )
            .into_response();
    }

    // 2. Verify and consume nonce
    if !crate::handlers::nonce::verify_and_consume_nonce(&state, &req.nonce) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid or expired nonce"})),
        )
            .into_response();
    }

    // 3. Verify DID signature proving ownership
    let message = format!(
        "issue_vc:{}:{}:{}",
        req.merchant_did, req.merchant_name, req.nonce
    );
    if !verify_did_signature(&req.merchant_did, &message, &req.did_signature) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid DID signature"})),
        )
            .into_response();
    }

    // 4. If merchant is already cached, this is an update — verify status
    let did_hash = compute_did_hash(&req.merchant_did);
    if let Some(cached) = state.get_cached_merchant(&did_hash) {
        // Merchant exists — verify the requester is the current controller
        let did_pk_bytes = match crate::did::resolver::extract_pubkey_from_did(&req.merchant_did) {
            Some(pk) => pk,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "cannot extract pubkey from DID"})),
                )
                    .into_response();
            }
        };
        let did_pk = solana_sdk::pubkey::Pubkey::new_from_array(did_pk_bytes);
        if cached.controller_pk != did_pk && cached.original_pk != did_pk {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "not authorized: signer is not controller or original key"})),
            )
                .into_response();
        }
    }

    // 5. Build credential fields
    let vc_id = format!("urn:uuid:{}", uuid::Uuid::new_v4());
    let validity_hours = req.validity_hours.unwrap_or(8760);
    let now = chrono::Utc::now();
    let expiration = now + chrono::Duration::hours(validity_hours as i64);
    let verification_method = format!("{}#key-signing-1", state.platform_did);

    // 6. Sign the VC with the platform key (includes credentialStatus for revocation)
    let vc = VerifiableCredential::sign(
        vec![
            "https://www.w3.org/2018/credentials/v1".to_string(),
            "https://ignite-pay.com/credentials/v1".to_string(),
        ],
        vc_id,
        vec![
            "VerifiableCredential".to_string(),
            "MerchantAttestation".to_string(),
        ],
        state.platform_did.clone(),
        now,
        expiration,
        req.merchant_did,
        req.merchant_name,
        req.category,
        &state.platform_signing_key,
        &verification_method,
        &state.did_program_id().to_string(),
    );

    // 7. Compute VC hash for on-chain use
    let vc_json = serde_json::to_vec(&vc).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&vc_json);
    let hash = hasher.finalize();
    let vc_hash_hex = hex::encode(hash);

    // 8. Store the VC in sled
    let store = crate::storage::sled_store::MerchantStore::new((*state.db).clone());
    if let Err(e) = store.save_vc(&vc_hash_hex, &vc_json) {
        tracing::error!("Failed to store VC: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to store VC"})),
        )
            .into_response();
    }

    // 9. Return the VC and its hash
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "verifiable_credential": vc,
            "vc_hash": vc_hash_hex,
        })),
    )
        .into_response()
}
