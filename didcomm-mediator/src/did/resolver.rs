use std::sync::Arc;
use tokio::sync::RwLock;

use affinidi_messaging_didcomm::DIDCommAgent;
use affinidi_messaging_didcomm::identity::PrivateIdentity;

/// Wraps a DIDCommAgent for use as shared state across the mediator.
/// The agent manages the mediator's own identity (keys) and resolved peers.
#[derive(Clone)]
pub struct MediatorDidAgent {
    inner: Arc<RwLock<DIDCommAgent>>,
    mediator_did: String,
}

impl MediatorDidAgent {
    /// Create a new mediator DID agent with a generated identity.
    pub fn new(mediator_did: String) -> Self {
        let identity = PrivateIdentity::generate(&mediator_did);
        let mut agent = DIDCommAgent::new();
        agent.add_identity(identity);

        Self {
            inner: Arc::new(RwLock::new(agent)),
            mediator_did,
        }
    }

    pub fn mediator_did(&self) -> &str {
        &self.mediator_did
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
