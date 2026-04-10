use dashmap::DashMap;
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
/// Delegates to the shared ignite-pay-core implementation.
pub fn parse_ignite_did_document(did: &str, doc: &serde_json::Value) -> Option<ResolvedIdentity> {
    ignite_pay_core::parse_did_document(did, doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use affinidi_messaging_didcomm::crypto::key_agreement::PublicKeyAgreement;

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
