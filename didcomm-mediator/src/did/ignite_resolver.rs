use base64::Engine;
use dashmap::DashMap;
use affinidi_messaging_didcomm::crypto::key_agreement::PublicKeyAgreement;
use affinidi_messaging_didcomm::identity::ResolvedIdentity;

use crate::did::resolver::MediatorDidAgent;

/// In-memory registry mapping `did:ignite:*` DIDs to their resolved identities.
/// Peers register themselves via the peer-introduction protocol.
#[derive(Debug, Default)]
pub struct IgniteDidRegistry {
    peers: DashMap<String, ResolvedIdentity>,
}

impl IgniteDidRegistry {
    pub fn new() -> Self {
        Self {
            peers: DashMap::new(),
        }
    }

    /// Register a peer's resolved identity.
    pub fn register_peer(&self, identity: ResolvedIdentity) {
        let did = identity.did.clone();
        self.peers.insert(did, identity);
    }

    /// Resolve a `did:ignite:*` DID to its resolved identity.
    pub fn resolve(&self, did: &str) -> Option<ResolvedIdentity> {
        self.peers.get(did).map(|r| r.value().clone())
    }

    /// Register a peer into both the registry and agent.
    /// Takes a cloneable ResolvedIdentity and stores it in both places.
    pub async fn register_peer_full(&self, identity: ResolvedIdentity, agent: &MediatorDidAgent) {
        let did = identity.did.clone();
        self.peers.insert(did, identity.clone());
        agent.write().await.add_peer(identity);
    }
}

/// Parse a DID Document JSON into a ResolvedIdentity for the `did:ignite` method.
/// Extracts the keyAgreement key (X25519 public key base64) and
/// optionally the verification key (Ed25519 public key multibase).
pub fn parse_ignite_did_document(did: &str, doc: &serde_json::Value) -> Option<ResolvedIdentity> {
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
    fn test_registry_register_and_resolve() {
        let registry = IgniteDidRegistry::new();
        let resolved = ResolvedIdentity::new(
            "did:ignite:test".to_string(),
            "did:ignite:test#key-agreement-1".to_string(),
            PublicKeyAgreement::X25519([0u8; 32]),
        );

        registry.register_peer(resolved.clone());
        let found = registry.resolve("did:ignite:test").unwrap();
        assert_eq!(found.did, "did:ignite:test");
    }

    #[test]
    fn test_registry_missing_did() {
        let registry = IgniteDidRegistry::new();
        assert!(registry.resolve("did:ignite:nonexistent").is_none());
    }
}
