use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use ignite_pay_core::types::VerifiableCredential;
use serde::Deserialize;
use sha2::{Digest, Sha256};

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
}

/// `POST /v1/vc/issue` — Issue a W3C Verifiable Credential for a merchant.
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

    // 3. Optionally verify merchant is registered (check sled cache)
    let did_hash = crate::did::resolver::compute_did_hash(&req.merchant_did);
    if state.get_cached_merchant(&did_hash).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "merchant not registered"})),
        )
            .into_response();
    }

    // 4. Build credential fields
    let vc_id = format!("urn:uuid:{}", uuid::Uuid::new_v4());
    let validity_hours = req.validity_hours.unwrap_or(8760);
    let now = chrono::Utc::now();
    let expiration = now + chrono::Duration::hours(validity_hours as i64);
    let verification_method = format!("{}#key-signing-1", state.platform_did);

    // 5. Sign the VC with the platform key
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
    );

    // 6. Compute VC hash for on-chain use
    let vc_json = serde_json::to_vec(&vc).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&vc_json);
    let hash = hasher.finalize();
    let vc_hash_hex = hex::encode(hash);

    // 7. Store the VC in sled
    let store = crate::storage::sled_store::MerchantStore::new((*state.db).clone());
    if let Err(e) = store.save_vc(&vc_hash_hex, &vc_json) {
        tracing::error!("Failed to store VC: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to store VC"})),
        )
            .into_response();
    }

    // 8. Return the VC and its hash
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "verifiable_credential": vc,
            "vc_hash": vc_hash_hex,
        })),
    )
        .into_response()
}
