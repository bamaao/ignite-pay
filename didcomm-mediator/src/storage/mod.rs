pub mod in_memory;

use async_trait::async_trait;
use std::collections::HashSet;

/// A queued DIDComm message waiting for pickup or online delivery.
#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub id: String,
    pub sender_did: String,
    pub recipient_did: String,
    pub encrypted_envelope: String,
    pub queued_at: chrono::DateTime<chrono::Utc>,
}

/// Persistence for queued messages.
#[async_trait]
pub trait MessageStore: Send + Sync {
    async fn enqueue(&self, recipient_did: &str, msg: QueuedMessage) -> crate::error::Result<()>;
    async fn dequeue_batch(
        &self,
        recipient_did: &str,
        limit: usize,
    ) -> crate::error::Result<Vec<QueuedMessage>>;
    async fn count(&self, recipient_did: &str) -> crate::error::Result<usize>;
    async fn remove(&self, msg_id: &str) -> crate::error::Result<()>;
}

/// Keylist (routing-table) mapping: which recipient DIDs this mediator routes for.
#[async_trait]
pub trait KeylistStore: Send + Sync {
    async fn add_key(&self, session_did: &str, recipient_did: &str) -> crate::error::Result<()>;
    async fn remove_key(&self, session_did: &str, recipient_did: &str) -> crate::error::Result<()>;
    async fn list_keys(&self, session_did: &str) -> crate::error::Result<HashSet<String>>;
    async fn resolve_session(&self, recipient_did: &str) -> crate::error::Result<Option<String>>;
}
