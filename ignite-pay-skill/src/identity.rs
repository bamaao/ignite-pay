use affinidi_messaging_didcomm::crypto::key_agreement::PublicKeyAgreement;
use affinidi_messaging_didcomm::identity::{PrivateIdentity, ResolvedIdentity};
use base64::Engine;
use serde_json::{json, Value};

/// Generate a new `did:ignite:<multibase>` identity.
///
/// Creates an Ed25519 keypair, derives the multibase identifier from the
/// verifying (public) key, and returns the private identity + DID string.
pub fn generate_ignite_did() -> (PrivateIdentity, String) {
    // We generate a temporary identity first to get the raw Ed25519 seed
    // and derive the public key for the DID identifier.
    let temp = PrivateIdentity::generate("did:ignite:temp");
    let verifying = temp.verifying_key().expect("signing key must be present");

    // Encode as multibase base58btc with multicodec Ed25519 public key prefix (0xed)
    // The did:key format: z + base58btc(0xed + pub_key_bytes)
    let did = encode_did_ignite(&verifying);

    // Now generate the real identity with the proper DID
    let identity = PrivateIdentity::generate(&did);
    (identity, did)
}

/// Encode an Ed25519 public key as a `did:ignite:z6Mk...` DID.
fn encode_did_ignite(pub_key: &[u8; 32]) -> String {
    // Multicodec Ed25519 public key: 0xed (varint) + 32 bytes
    let mut prefixed = vec![0xed, 0x01];
    prefixed.extend_from_slice(pub_key);

    // Base58-btc encode (the 'z' prefix is the multibase base58btc indicator)
    let encoded = bs58::encode(&prefixed).into_string();
    format!("did:ignite:z{}", encoded)
}

/// Build a W3C DID Document for the `did:ignite` method.
pub fn build_did_document(did: &str, identity: &PrivateIdentity) -> Value {
    let key_agreement_kid = format!("{}#key-agreement-1", did);
    let signing_kid = format!("{}#key-signing-1", did);

    // Get the public key agreement key as raw bytes (X25519)
    let ka_public = identity.public_key_agreement();
    let ka_bytes = match ka_public {
        PublicKeyAgreement::X25519(bytes) => bytes,
        _ => panic!("unsupported key agreement curve"),
    };
    let ka_b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode(ka_bytes);

    // Get the verifying key as raw bytes (Ed25519)
    let vk_bytes = identity.verifying_key().expect("signing key");

    json!({
        "@context": ["https://www.w3.org/ns/did/v1"],
        "id": did,
        "verificationMethod": [{
            "id": signing_kid,
            "type": "Ed25519VerificationKey2020",
            "controller": did,
            "publicKeyMultibase": format!("z{}", bs58::encode([0xed, 0x01].iter().chain(vk_bytes.iter()).copied().collect::<Vec<_>>()).into_string())
        }],
        "keyAgreement": [{
            "id": key_agreement_kid,
            "type": "X25519KeyAgreementKey2020",
            "controller": did,
            "publicKeyBase64": ka_b64
        }]
    })
}

/// Convert a PrivateIdentity into a ResolvedIdentity for peer registration.
pub fn identity_to_resolved(identity: &PrivateIdentity) -> ResolvedIdentity {
    identity.to_resolved()
}

/// Parse a DID Document JSON into a ResolvedIdentity.
#[allow(dead_code)]
/// Extracts the keyAgreement key (X25519 public key base64) and
/// verification key (Ed25519 public key multibase).
pub fn parse_did_document(did: &str, doc: &Value) -> Option<ResolvedIdentity> {
    use affinidi_messaging_didcomm::crypto::key_agreement::PublicKeyAgreement;

    // Extract key agreement key
    let ka_entry = doc.get("keyAgreement")?.as_array()?.first()?;
    let ka_kid = ka_entry.get("id")?.as_str()?;
    let ka_b64 = ka_entry.get("publicKeyBase64")?.as_str()?;
    let ka_bytes = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(ka_b64)
        .ok()?;
    if ka_bytes.len() != 32 {
        return None;
    }
    let mut ka_arr = [0u8; 32];
    ka_arr.copy_from_slice(&ka_bytes);

    let mut resolved = ResolvedIdentity::new(
        did.to_string(),
        ka_kid.to_string(),
        PublicKeyAgreement::X25519(ka_arr),
    );

    // Extract verification key (optional)
    if let Some(vm) = doc.get("verificationMethod").and_then(|v| v.as_array()) {
        for method in vm {
            if let Some(pk_multibase) = method.get("publicKeyMultibase").and_then(|v| v.as_str()) {
                if pk_multibase.starts_with('z') {
                    // Decode base58btc, skip multicodec prefix (0xed 0x01)
                    if let Ok(decoded) = bs58::decode(&pk_multibase[1..]).into_vec() {
                        if decoded.len() == 34 && decoded[0] == 0xed && decoded[1] == 0x01 {
                            let mut vk = [0u8; 32];
                            vk.copy_from_slice(&decoded[2..34]);
                            let kid = method.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            resolved.signing_kid = Some(kid);
                            resolved.verifying_key = Some(vk);
                        }
                    }
                }
            }
        }
    }

    Some(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_identity() {
        let (identity, did) = generate_ignite_did();
        assert!(did.starts_with("did:ignite:z"));
        assert!(did.len() > "did:ignite:z".len() + 10);
        assert_eq!(identity.did, did);
    }

    #[test]
    fn test_build_did_document_roundtrip() {
        let (identity, did) = generate_ignite_did();
        let doc = build_did_document(&did, &identity);

        assert_eq!(doc["id"], did);

        // Round-trip: parse the document back
        let resolved = parse_did_document(&did, &doc).expect("parse failed");
        assert_eq!(resolved.did, did);
        assert!(resolved.signing_kid.is_some());
        assert!(resolved.verifying_key.is_some());
    }

    #[test]
    fn test_identity_to_resolved() {
        let (identity, did) = generate_ignite_did();
        let resolved = identity_to_resolved(&identity);
        assert_eq!(resolved.did, did);
        assert!(resolved.key_agreement_kid.contains("key-agreement-1"));
    }
}
