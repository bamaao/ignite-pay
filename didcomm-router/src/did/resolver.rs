use std::sync::Arc;
use tokio::sync::RwLock;

use affinidi_messaging_didcomm::DIDCommAgent;
use affinidi_messaging_didcomm::identity::PrivateIdentity;
use serde_json::Value;

/// Wraps a DIDCommAgent for use as shared state across the router.
/// The agent manages the router's own identity (keys) and resolved peers.
#[derive(Clone)]
pub struct RouterDidAgent {
    inner: Arc<RwLock<DIDCommAgent>>,
    router_did: String,
    did_doc: Arc<Value>,
}

impl RouterDidAgent {
    /// Create a new router DID agent with a generated identity.
    pub fn new(router_did: String) -> Self {
        let identity = PrivateIdentity::generate(&router_did);
        let did_doc = ignite_pay_core::build_did_document(&router_did, &identity);
        let mut agent = DIDCommAgent::new();
        agent.add_identity(identity);

        Self {
            inner: Arc::new(RwLock::new(agent)),
            router_did,
            did_doc: Arc::new(did_doc),
        }
    }

    pub fn router_did(&self) -> &str {
        &self.router_did
    }

    /// Get the cached DID document JSON for this router.
    pub fn did_doc(&self) -> &Value {
        &self.did_doc
    }

    /// Get read access to the inner agent.
    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, DIDCommAgent> {
        self.inner.read().await
    }

    /// Get write access to the inner agent.
    pub async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, DIDCommAgent> {
        self.inner.write().await
    }
}
