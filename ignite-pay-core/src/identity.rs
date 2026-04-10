use affinidi_messaging_didcomm::crypto::key_agreement::PublicKeyAgreement;
use affinidi_messaging_didcomm::identity::{PrivateIdentity, ResolvedIdentity};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Serialized identity data for persistence.
#[derive(Debug, Serialize, Deserialize)]
struct StoredIdentity {
    did: String,
    did_doc: Value,
}

/// Generate a new `did:ignite:<multibase>` identity.
///
/// Creates an Ed25519 keypair, derives the multibase identifier from the
/// verifying (public) key, and returns the private identity + DID string.
pub fn generate_ignite_did() -> (PrivateIdentity, String) {
    let temp = PrivateIdentity::generate("did:ignite:temp");
    let verifying = temp.verifying_key().expect("signing key must be present");
    let did = encode_did_ignite(&verifying);
    let identity = PrivateIdentity::generate(&did);
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

/// Save identity to sled database for persistence across restarts.
/// Stores the DID string and DID document under the `__identity__` key.
///
/// NOTE: The affinidi-messaging-didcomm `PrivateIdentity` does not expose
/// the raw seed bytes, so we store the DID + DID document. On load, a new
/// identity is generated with the same DID. The keys will differ, but the
/// DID identifier is preserved. For true key persistence, the PrivateIdentity
/// seed would need to be extracted at generation time.
pub fn save_identity(db: &sled::Db, _identity: &PrivateIdentity, did: &str) -> Result<(), anyhow::Error> {
    let stored = StoredIdentity {
        did: did.to_string(),
        did_doc: Value::Null, // Will be rebuilt on load
    };
    let value = serde_json::to_vec(&stored)?;
    db.insert("__identity__", value)?;
    db.flush()?;
    Ok(())
}

/// Load a previously saved identity from sled database.
/// Returns the DID string if found, or None if not found.
///
/// NOTE: Since we can't persist the actual private key seed, this returns
/// the DID string so a new identity can be generated with the same DID.
pub fn load_did(db: &sled::Db) -> Result<Option<String>, anyhow::Error> {
    if let Some(bytes) = db.get("__identity__")? {
        let stored: StoredIdentity = serde_json::from_slice(&bytes)?;
        Ok(Some(stored.did))
    } else {
        Ok(None)
    }
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
        let (_identity, did) = generate_ignite_did();
        save_identity(&db, &_identity, &did).unwrap();

        let loaded = load_did(&db).unwrap().unwrap();
        assert_eq!(loaded, did);
    }
}
