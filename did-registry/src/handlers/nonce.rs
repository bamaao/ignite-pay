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

use crate::state::RegistryState;

/// `GET /v1/auth/nonce` — Issue a server nonce for replay protection.
///
/// The client includes this nonce in the signed message when calling
/// rotate-key, update-vc, or register. Nonces expire after 5 minutes.
pub async fn issue_nonce(State(state): State<RegistryState>) -> impl IntoResponse {
    // Prune expired nonces periodically
    let now = chrono::Utc::now().timestamp();
    if state.nonces.len() > 10_000 {
        state.nonces.retain(|_, &mut expiry| expiry > now);
    }

    let nonce = uuid::Uuid::new_v4().to_string();
    let ttl_secs: i64 = 300; // 5 minutes
    state.nonces.insert(nonce.clone(), now + ttl_secs);

    (StatusCode::OK, axum::Json(serde_json::json!({
        "nonce": nonce,
        "expires_in": ttl_secs,
    })))
}

/// Verify and consume a previously issued nonce.
/// Returns true if the nonce was valid and has been consumed (removed).
pub fn verify_and_consume_nonce(state: &RegistryState, nonce: &str) -> bool {
    let now = chrono::Utc::now().timestamp();

    match state.nonces.remove(nonce) {
        Some((_, expiry)) => expiry > now,
        None => false,
    }
}
