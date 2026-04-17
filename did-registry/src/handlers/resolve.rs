use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use tracing::info;

use crate::did::resolver::compute_did_hash;
use crate::state::RegistryState;

/// `GET /v1/did/resolve/{did}` — Resolve a `did:ignite` DID to its DID Document.
pub async fn resolve_did(
    State(state): State<RegistryState>,
    Path(did): Path<String>,
) -> impl IntoResponse {
    if !did.starts_with("did:ignite:") {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": "Only did:ignite DIDs are supported" })),
        )
            .into_response();
    }

    info!("Resolving DID: {}", did);

    // Build a DID Document from the DID string by extracting the public key
    let did_doc = build_basic_did_document(&did);

    // Enrich with on-chain data if available
    let did_hash = compute_did_hash(&did);
    if let Some(cached_did) = state.get_cached_merchant(&did_hash) {
        let mut doc = did_doc;
        if let Some(obj) = doc.as_object_mut() {
            obj.insert(
                "controller_pubkey".to_string(),
                serde_json::Value::String(cached_did.controller_pk.to_string()),
            );
            obj.insert(
                "original_pubkey".to_string(),
                serde_json::Value::String(cached_did.original_pk.to_string()),
            );
            obj.insert(
                "last_updated".to_string(),
                serde_json::Value::Number(cached_did.last_updated.into()),
            );
        }
        return (StatusCode::OK, axum::Json(doc)).into_response();
    }

    // Return basic DID Document without on-chain enrichment
    (StatusCode::OK, axum::Json(did_doc)).into_response()
}

/// Build a basic DID Document from a did:ignite identifier.
/// Extracts the Ed25519 public key from the multibase-encoded identifier.
fn build_basic_did_document(did: &str) -> serde_json::Value {
    let prefix = "did:ignite:z";
    let signing_kid = format!("{}#key-signing-1", did);

    let public_key_multibase = if did.starts_with(prefix) {
        // The part after 'z' is already base58-encoded multicodec
        format!("z{}", &did[prefix.len()..])
    } else {
        "unknown".to_string()
    };

    serde_json::json!({
        "@context": ["https://www.w3.org/ns/did/v1"],
        "id": did,
        "verificationMethod": [{
            "id": signing_kid,
            "type": "Ed25519VerificationKey2020",
            "controller": did,
            "publicKeyMultibase": public_key_multibase
        }]
    })
}
