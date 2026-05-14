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

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use tracing::info;

use crate::did::resolver::compute_did_hash;
use crate::state::RegistryState;
use crate::storage::sled_store::MerchantStore;

/// `GET /v1/merchants/profile/{did}` — Return merchant profile from DID Registry.
pub async fn merchant_profile(
    State(state): State<RegistryState>,
    Path(did): Path<String>,
) -> impl IntoResponse {
    if !did.starts_with("did:ignite:") {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": "Invalid DID format" })),
        )
            .into_response();
    }

    info!("Fetching merchant profile: {}", did);

    let did_hash = compute_did_hash(&did);

    let cached_did = match state.get_cached_merchant(&did_hash) {
        Some(account) => account,
        None => {
            return (
                StatusCode::NOT_FOUND,
                axum::Json(serde_json::json!({ "error": "Merchant not found" })),
            )
                .into_response();
        }
    };

    // Check if vc_hash is all zeros (no VC issued)
    if cached_did.vc_hash.iter().all(|&b| b == 0) {
        return (StatusCode::OK, axum::Json(serde_json::json!({
            "did": did,
            "verified": false,
            "name": null,
            "category": null,
        })))
        .into_response();
    }

    // Look up the VC by its hash
    let store = MerchantStore::new((*state.db).clone());
    let vc_hash_hex = hex::encode(cached_did.vc_hash);

    let vc_bytes = match store.get_vc(&vc_hash_hex) {
        Some(bytes) => bytes,
        None => {
            // Merchant found but VC data missing from store
            return (StatusCode::OK, axum::Json(serde_json::json!({
                "did": did,
                "verified": false,
                "name": null,
                "category": null,
            })))
            .into_response();
        }
    };

    // Deserialize VC JSON and extract credential_subject
    let vc: serde_json::Value = match serde_json::from_slice(&vc_bytes) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to deserialize VC for {}: {}", did, e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": "Failed to parse VC" })),
            )
                .into_response();
        }
    };

    let subject = vc.get("credential_subject");
    let name = subject
        .and_then(|s| s.get("name"))
        .and_then(|n| n.as_str())
        .map(String::from);
    let category = subject
        .and_then(|s| s.get("category"))
        .and_then(|c| c.as_str())
        .map(String::from);

    (StatusCode::OK, axum::Json(serde_json::json!({
        "did": did,
        "verified": true,
        "name": name,
        "category": category,
    })))
    .into_response()
}
