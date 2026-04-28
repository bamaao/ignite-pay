use affinidi_messaging_didcomm::crypto::key_agreement::{Curve, PrivateKeyAgreement, PublicKeyAgreement};
use affinidi_messaging_didcomm::identity::{PrivateIdentity, ResolvedIdentity};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Serialized identity data for persistence.
/// Stores DID + raw private key bytes so keys survive restart.
#[derive(Debug, Serialize, Deserialize)]
struct StoredIdentity {
    did: String,
    ed25519_signing_private: [u8; 32],
    x25519_key_agreement_private: [u8; 32],
}

/// Generate a new `did:ignite:<multibase>` identity.
///
/// Creates an Ed25519 keypair, derives the multibase identifier from the
/// verifying (public) key, and returns the private identity + DID string.
pub fn generate_ignite_did() -> (PrivateIdentity, String) {
    // Generate once with a placeholder DID, then derive the real DID from its key.
    let mut identity = PrivateIdentity::generate("did:ignite:temp");
    let verifying = identity.verifying_key().expect("signing key must be present");
    let did = encode_did_ignite(&verifying);
    // Patch the identity's DID to match the derived identifier so the keys stay consistent.
    identity.did = did.clone();
    identity.key_agreement_kid = format!("{}#key-agreement-1", did);
    identity.signing_kid = Some(format!("{}#key-signing-1", did));
    (identity, did)
}

/// Encode an Ed25519 public key as a `did:ignite:z6Mk...` DID.
fn encode_did_ignite(pub_key: &[u8; 32]) -> String {
    let mut prefixed = vec![0xed, 0x01];
    prefixed.extend_from_slice(pub_key);
    let encoded = bs58::encode(&prefixed).into_string();
    format!("did:ignite:z{}", encoded)
}

/// Build a W3C DID Document for the `did:ignite` method.
pub fn build_did_document(did: &str, identity: &PrivateIdentity) -> Value {
    let key_agreement_kid = format!("{}#key-agreement-1", did);
    let signing_kid = format!("{}#key-signing-1", did);

    let ka_public = identity.public_key_agreement();
    let ka_bytes = match ka_public {
        PublicKeyAgreement::X25519(bytes) => bytes,
        _ => panic!("unsupported key agreement curve"),
    };
    let ka_b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode(ka_bytes);

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
/// Extracts the keyAgreement key (X25519 public key base64) and
/// verification key (Ed25519 public key multibase).
pub fn parse_did_document(did: &str, doc: &Value) -> Option<ResolvedIdentity> {
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

    if let Some(vm) = doc.get("verificationMethod").and_then(|v| v.as_array()) {
        for method in vm {
            if let Some(pk_multibase) = method.get("publicKeyMultibase").and_then(|v| v.as_str()) {
                if pk_multibase.starts_with('z') {
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

/// Extract the X25519 key agreement private key bytes from a PrivateIdentity.
fn extract_ka_private(identity: &PrivateIdentity) -> [u8; 32] {
    match &identity.key_agreement_private {
        PrivateKeyAgreement::X25519(secret) => secret.to_bytes(),
        _ => panic!("unsupported key agreement curve"),
    }
}

/// Save identity to sled database for persistence across restarts.
/// Stores the DID string and both private keys under the `__identity__` key.
pub fn save_identity(db: &sled::Db, identity: &PrivateIdentity, did: &str) -> Result<(), anyhow::Error> {
    let signing_bytes = identity.signing_private.ok_or_else(|| anyhow::anyhow!("no signing key"))?;
    let ka_bytes = extract_ka_private(identity);
    let stored = StoredIdentity {
        did: did.to_string(),
        ed25519_signing_private: signing_bytes,
        x25519_key_agreement_private: ka_bytes,
    };
    let value = serde_json::to_vec(&stored)?;
    db.insert("__identity__", value)?;
    db.flush()?;
    Ok(())
}

/// Load a previously saved identity from sled database.
/// Reconstructs the full PrivateIdentity with the same private keys.
/// Returns None if no identity was previously saved.
pub fn load_identity(db: &sled::Db) -> Result<Option<PrivateIdentity>, anyhow::Error> {
    if let Some(bytes) = db.get("__identity__")? {
        let stored: StoredIdentity = serde_json::from_slice(&bytes)?;
        let identity = PrivateIdentity {
            did: stored.did.clone(),
            key_agreement_kid: format!("{}#key-agreement-1", stored.did),
            key_agreement_private: PrivateKeyAgreement::from_raw_bytes(Curve::X25519, &stored.x25519_key_agreement_private)
                .map_err(|e| anyhow::anyhow!("failed to reconstruct key agreement key: {:?}", e))?,
            signing_kid: Some(format!("{}#key-signing-1", stored.did)),
            signing_private: Some(stored.ed25519_signing_private),
        };
        Ok(Some(identity))
    } else {
        Ok(None)
    }
}

/// Load only the DID string (convenience wrapper around load_identity).
pub fn load_did(db: &sled::Db) -> Result<Option<String>, anyhow::Error> {
    Ok(load_identity(db)?.map(|id| id.did))
}

/// Extract the Ed25519 public key bytes from a did:ignite identifier.
/// Format: did:ignite:z + Base58(0xed 0x01 + Ed25519_pubkey)
pub fn extract_pubkey_from_did(did: &str) -> Option<[u8; 32]> {
    let prefix = "did:ignite:z";
    if !did.starts_with(prefix) {
        return None;
    }

    let encoded = &did[prefix.len()..];
    let decoded = bs58::decode(encoded).into_vec().ok()?;

    // Expect multicodec prefix 0xed 0x01 + 32 bytes Ed25519 pubkey
    if decoded.len() != 34 || decoded[0] != 0xed || decoded[1] != 0x01 {
        return None;
    }

    let mut pk = [0u8; 32];
    pk.copy_from_slice(&decoded[2..34]);
    Some(pk)
}

/// Sign a message with an Ed25519 signing key, returning base64-no-pad encoded signature.
/// `signing_private` is the 32-byte Ed25519 private key (seed).
pub fn sign_message(signing_private: &[u8; 32], message: &[u8]) -> String {
    use ed25519_dalek::Signer;
    let signing_key = ed25519_dalek::SigningKey::from_bytes(signing_private);
    let signature = signing_key.sign(message);
    base64::engine::general_purpose::STANDARD_NO_PAD.encode(signature.to_bytes())
}

/// Verify an Ed25519 signature from a did:ignite DID key.
/// Extracts the public key from the DID and verifies the signature over the message.
pub fn verify_did_signature(did: &str, message: &str, signature_b64: &str) -> bool {
    let pk_bytes = match extract_pubkey_from_did(did) {
        Some(bytes) => bytes,
        None => return false,
    };

    let verifying_key = match ed25519_dalek::VerifyingKey::from_bytes(&pk_bytes) {
        Ok(key) => key,
        Err(_) => return false,
    };

    let signature_bytes = match base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(signature_b64)
    {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };

    if signature_bytes.len() != 64 {
        return false;
    }

    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&signature_bytes);
    let sig = match ed25519_dalek::Signature::try_from(sig_arr.as_slice()) {
        Ok(s) => s,
        Err(_) => return false,
    };

    use ed25519_dalek::Verifier;
    verifying_key.verify(message.as_bytes(), &sig).is_ok()
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

    #[test]
    fn test_save_load_identity() {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let (identity, did) = generate_ignite_did();
        save_identity(&db, &identity, &did).unwrap();

        let loaded = load_identity(&db).unwrap().unwrap();
        assert_eq!(loaded.did, did);
        // Keys should match exactly
        assert_eq!(loaded.signing_private, identity.signing_private);
        let loaded_ka = match &loaded.key_agreement_private {
            PrivateKeyAgreement::X25519(s) => s.to_bytes(),
            _ => panic!("unexpected curve"),
        };
        let orig_ka = match &identity.key_agreement_private {
            PrivateKeyAgreement::X25519(s) => s.to_bytes(),
            _ => panic!("unexpected curve"),
        };
        assert_eq!(loaded_ka, orig_ka);
    }

    #[test]
    fn test_extract_pubkey_roundtrip() {
        // Generate a keypair, build a DID from it, then verify extraction
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]);
        let vk = signing_key.verifying_key();
        let did = format!(
            "did:ignite:z{}",
            bs58::encode([0xed, 0x01].iter().chain(vk.as_bytes().iter()).copied().collect::<Vec<_>>()).into_string()
        );
        let extracted = extract_pubkey_from_did(&did).expect("extract failed");
        assert_eq!(extracted, *vk.as_bytes());
    }

    #[test]
    fn test_verify_did_signature_roundtrip() {
        // Use a known signing key, build DID from its public key, sign and verify
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]);
        let vk = signing_key.verifying_key();
        let did = format!(
            "did:ignite:z{}",
            bs58::encode([0xed, 0x01].iter().chain(vk.as_bytes().iter()).copied().collect::<Vec<_>>()).into_string()
        );
        let message = "test-message";
        use ed25519_dalek::Signer;
        let sig = signing_key.sign(message.as_bytes());
        let sig_b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode(sig.to_bytes());
        assert!(verify_did_signature(&did, message, &sig_b64));
        assert!(!verify_did_signature(&did, "wrong-message", &sig_b64));
    }

    // --- parse_did_document edge cases ---

    #[test]
    fn test_parse_did_document_missing_key_agreement() {
        let doc = json!({
            "id": "did:test:1",
            "verificationMethod": []
        });
        assert!(parse_did_document("did:test:1", &doc).is_none());
    }

    #[test]
    fn test_parse_did_document_missing_verification_method() {
        let (identity, did) = generate_ignite_did();
        let mut doc = build_did_document(&did, &identity);
        // Remove verificationMethod
        doc.as_object_mut().unwrap().remove("verificationMethod");
        // Should still parse (verificationMethod is optional)
        let resolved = parse_did_document(&did, &doc).unwrap();
        assert_eq!(resolved.did, did);
        assert!(resolved.verifying_key.is_none());
    }

    #[test]
    fn test_parse_did_document_invalid_base64_key() {
        let doc = json!({
            "id": "did:test:1",
            "keyAgreement": [{
                "id": "did:test:1#key-agreement-1",
                "type": "X25519KeyAgreementKey2020",
                "controller": "did:test:1",
                "publicKeyBase64": "!!!invalid!!!"
            }],
            "verificationMethod": []
        });
        assert!(parse_did_document("did:test:1", &doc).is_none());
    }

    #[test]
    fn test_parse_did_document_wrong_key_length() {
        let short_key = base64::engine::general_purpose::STANDARD_NO_PAD.encode([0u8; 16]);
        let doc = json!({
            "id": "did:test:1",
            "keyAgreement": [{
                "id": "did:test:1#key-agreement-1",
                "type": "X25519KeyAgreementKey2020",
                "controller": "did:test:1",
                "publicKeyBase64": short_key
            }],
            "verificationMethod": []
        });
        assert!(parse_did_document("did:test:1", &doc).is_none());
    }

    #[test]
    fn test_parse_did_document_empty_key_agreement_array() {
        let doc = json!({
            "id": "did:test:1",
            "keyAgreement": [],
            "verificationMethod": []
        });
        assert!(parse_did_document("did:test:1", &doc).is_none());
    }

    #[test]
    fn test_parse_did_document_invalid_multibase_prefix() {
        let (identity, did) = generate_ignite_did();
        let mut doc = build_did_document(&did, &identity);
        // Corrupt the multibase key (remove the 'z' prefix)
        if let Some(vm) = doc.get_mut("verificationMethod").unwrap().as_array_mut() {
            if let Some(key) = vm[0].get_mut("publicKeyMultibase") {
                *key = json!("abc123"); // no 'z' prefix
            }
        }
        // Should still parse key agreement, but verifying_key should be None
        let resolved = parse_did_document(&did, &doc).unwrap();
        assert!(resolved.verifying_key.is_none());
    }

    #[test]
    fn test_parse_did_document_roundtrip_full() {
        let (identity, did) = generate_ignite_did();
        let doc = build_did_document(&did, &identity);
        let resolved = parse_did_document(&did, &doc).unwrap();

        assert_eq!(resolved.did, did);
        assert!(resolved.key_agreement_kid.contains("key-agreement-1"));
        assert!(resolved.signing_kid.unwrap().contains("key-signing-1"));
        assert!(resolved.verifying_key.is_some());

        // The verifying key should match the original
        let original_vk = identity.verifying_key().unwrap();
        assert_eq!(resolved.verifying_key.unwrap(), original_vk);
    }

    // --- extract_pubkey_from_did edge cases ---

    #[test]
    fn test_extract_pubkey_wrong_prefix() {
        assert!(extract_pubkey_from_did("did:key:z1234").is_none());
        assert!(extract_pubkey_from_did("did:web:example.com").is_none());
    }

    #[test]
    fn test_extract_pubkey_empty_string() {
        assert!(extract_pubkey_from_did("").is_none());
    }

    #[test]
    fn test_extract_pubkey_invalid_multicodec() {
        // Encode with wrong multicodec prefix
        let encoded = bs58::encode([0x12, 0x20].iter().chain([0u8; 32].iter()).copied().collect::<Vec<_>>()).into_string();
        let did = format!("did:ignite:z{}", encoded);
        assert!(extract_pubkey_from_did(&did).is_none());
    }

    #[test]
    fn test_extract_pubkey_too_short() {
        // Only 2 bytes (prefix), no key data
        let encoded = bs58::encode([0xed, 0x01]).into_string();
        let did = format!("did:ignite:z{}", encoded);
        assert!(extract_pubkey_from_did(&did).is_none());
    }

    // --- verify_did_signature edge cases ---

    #[test]
    fn test_verify_signature_wrong_did_format() {
        assert!(!verify_did_signature("did:web:example.com", "msg", "sig"));
    }

    #[test]
    fn test_verify_signature_invalid_base64_sig() {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]);
        let vk = signing_key.verifying_key();
        let did = format!(
            "did:ignite:z{}",
            bs58::encode([0xed, 0x01].iter().chain(vk.as_bytes().iter()).copied().collect::<Vec<_>>()).into_string()
        );
        assert!(!verify_did_signature(&did, "msg", "!!!invalid!!!"));
    }

    #[test]
    fn test_verify_signature_wrong_signature_length() {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]);
        let vk = signing_key.verifying_key();
        let did = format!(
            "did:ignite:z{}",
            bs58::encode([0xed, 0x01].iter().chain(vk.as_bytes().iter()).copied().collect::<Vec<_>>()).into_string()
        );
        let short_sig = base64::engine::general_purpose::STANDARD_NO_PAD.encode([0u8; 32]);
        assert!(!verify_did_signature(&did, "msg", &short_sig));
    }

    // --- load_identity edge cases ---

    #[test]
    fn test_load_identity_no_data() {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let result = load_identity(&db).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_load_did_no_data() {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let result = load_did(&db).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_load_did_returns_same_did() {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let (_, did) = generate_ignite_did();
        let (identity, _) = generate_ignite_did();
        // Save with first identity
        save_identity(&db, &identity, &did).unwrap();
        let loaded_did = load_did(&db).unwrap().unwrap();
        assert_eq!(loaded_did, did);
    }
}
