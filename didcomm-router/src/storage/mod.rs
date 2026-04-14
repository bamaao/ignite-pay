pub mod in_memory;

use async_trait::async_trait;
use std::collections::HashSet;

/// A queued DIDComm message waiting for pickup or online delivery.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

    /// Store a message for a user without dequeuing on pickup (persists until removed or TTL).
    async fn store_for_user(&self, recipient_did: &str, msg: QueuedMessage) -> crate::error::Result<()>;

    /// Get a single message by recipient DID and message ID.
    async fn get_message(&self, recipient_did: &str, msg_id: &str) -> crate::error::Result<Option<QueuedMessage>>;

    /// List messages for a recipient, optionally after a given message ID, with a limit.
    async fn list_messages(
        &self,
        recipient_did: &str,
        after_id: Option<&str>,
        limit: usize,
    ) -> crate::error::Result<Vec<QueuedMessage>>;
}

/// Persistence for device tokens (FCM registration).
#[async_trait]
pub trait DeviceTokenStore: Send + Sync {
    async fn register_device_token(&self, user_did: &str, fcm_token: &str) -> crate::error::Result<()>;
    async fn get_device_token(&self, user_did: &str) -> crate::error::Result<Option<String>>;
}

/// Keylist (routing-table) mapping: which recipient DIDs this router routes for.
#[async_trait]
pub trait KeylistStore: Send + Sync {
    async fn add_key(&self, session_did: &str, recipient_did: &str) -> crate::error::Result<()>;
    async fn remove_key(&self, session_did: &str, recipient_did: &str) -> crate::error::Result<()>;
    async fn list_keys(&self, session_did: &str) -> crate::error::Result<HashSet<String>>;
    async fn resolve_session(&self, recipient_did: &str) -> crate::error::Result<Option<String>>;
}

/// Agent-to-user binding: maps an agent DID to the user DID that owns it.
#[async_trait]
pub trait AgentBindingStore: Send + Sync {
    /// Bind an agent DID to a user did.
    async fn bind(&self, agent_did: &str, user_did: &str) -> crate::error::Result<()>;
    /// Look up the user DID that owns an agent.
    async fn get_user_for_agent(&self, agent_did: &str) -> crate::error::Result<Option<String>>;
    /// Look up all agent DIDs bound to a user.
    async fn get_agents_for_user(&self, user_did: &str) -> crate::error::Result<Vec<String>>;
}
