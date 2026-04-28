use async_trait::async_trait;
use dashmap::DashMap;
use std::collections::HashSet;
use std::sync::Arc;

use super::{AgentBindingStore, DeviceTokenStore, KeylistStore, MessageStore, QueuedMessage};

// ── In-memory MessageStore ──────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct InMemoryMessageStore {
    /// recipient_did -> Vec<QueuedMessage>
    queues: DashMap<String, Vec<QueuedMessage>>,
    max_queued: usize,
    max_message_age_secs: u64,
}

impl InMemoryMessageStore {
    pub fn new(max_queued: usize) -> Self {
        Self {
            queues: DashMap::new(),
            max_queued,
            max_message_age_secs: 86400, // default 24h
        }
    }

    pub fn with_max_age(max_queued: usize, max_message_age_secs: u64) -> Self {
        Self {
            queues: DashMap::new(),
            max_queued,
            max_message_age_secs,
        }
    }
}

#[async_trait]
impl MessageStore for InMemoryMessageStore {
    async fn enqueue(&self, recipient_did: &str, msg: QueuedMessage) -> crate::error::Result<()> {
        // Reject already-expired messages at enqueue time
        if msg.is_expired(self.max_message_age_secs) {
            return Ok(());
        }
        let mut queue = self.queues.entry(recipient_did.to_string()).or_default();
        if queue.len() >= self.max_queued {
            // Drop oldest message
            queue.remove(0);
        }
        queue.push(msg);
        Ok(())
    }

    async fn dequeue_batch(
        &self,
        recipient_did: &str,
        limit: usize,
    ) -> crate::error::Result<Vec<QueuedMessage>> {
        let mut queue = match self.queues.get_mut(recipient_did) {
            Some(q) => q,
            None => return Ok(Vec::new()),
        };
        // Drop expired messages and collect up to `limit` valid ones
        queue.retain(|m| !m.is_expired(self.max_message_age_secs));
        let take = limit.min(queue.len());
        let messages: Vec<QueuedMessage> = queue.drain(..take).collect();
        Ok(messages)
    }

    async fn count(&self, recipient_did: &str) -> crate::error::Result<usize> {
        Ok(self
            .queues
            .get(recipient_did)
            .map(|q| q.len())
            .unwrap_or(0))
    }

    async fn remove(&self, msg_id: &str) -> crate::error::Result<()> {
        for mut entry in self.queues.iter_mut() {
            entry.value_mut().retain(|m| m.id != msg_id);
        }
        Ok(())
    }

    async fn store_for_user(&self, recipient_did: &str, msg: QueuedMessage) -> crate::error::Result<()> {
        // Reject already-expired messages
        if msg.is_expired(self.max_message_age_secs) {
            return Ok(());
        }
        // store_for_user uses the same queue as enqueue — messages persist until explicitly removed.
        let mut queue = self.queues.entry(recipient_did.to_string()).or_default();
        if queue.len() >= self.max_queued {
            queue.remove(0);
        }
        queue.push(msg);
        Ok(())
    }

    async fn get_message(&self, recipient_did: &str, msg_id: &str) -> crate::error::Result<Option<QueuedMessage>> {
        Ok(self
            .queues
            .get(recipient_did)
            .and_then(|queue| queue.iter().find(|m| m.id == msg_id).cloned()))
    }

    async fn list_messages(
        &self,
        recipient_did: &str,
        after_id: Option<&str>,
        limit: usize,
    ) -> crate::error::Result<Vec<QueuedMessage>> {
        let queue = match self.queues.get(recipient_did) {
            Some(q) => q,
            None => return Ok(Vec::new()),
        };

        let messages: Vec<QueuedMessage> = match after_id {
            Some(after) => {
                let start = queue
                    .iter()
                    .position(|m| m.id == after)
                    .map(|pos| pos + 1)
                    .unwrap_or(0);
                queue.iter().skip(start).take(limit).cloned().collect()
            }
            None => queue.iter().take(limit).cloned().collect(),
        };

        Ok(messages)
    }
}

// ── In-memory KeylistStore ──────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct InMemoryKeylistStore {
    /// session_did -> set of routed recipient_dids
    keylists: DashMap<String, HashSet<String>>,
    /// reverse lookup: recipient_did -> session_did
    reverse: DashMap<String, String>,
}

#[async_trait]
impl KeylistStore for InMemoryKeylistStore {
    async fn add_key(&self, session_did: &str, recipient_did: &str) -> crate::error::Result<()> {
        self.keylists
            .entry(session_did.to_string())
            .or_default()
            .insert(recipient_did.to_string());
        self.reverse
            .insert(recipient_did.to_string(), session_did.to_string());
        Ok(())
    }

    async fn remove_key(&self, session_did: &str, recipient_did: &str) -> crate::error::Result<()> {
        if let Some(mut keys) = self.keylists.get_mut(session_did) {
            keys.remove(recipient_did);
        }
        self.reverse.remove(recipient_did);
        Ok(())
    }

    async fn list_keys(&self, session_did: &str) -> crate::error::Result<HashSet<String>> {
        Ok(self
            .keylists
            .get(session_did)
            .map(|k| k.value().clone())
            .unwrap_or_default())
    }

    async fn resolve_session(&self, recipient_did: &str) -> crate::error::Result<Option<String>> {
        Ok(self.reverse.get(recipient_did).map(|v| v.value().clone()))
    }

    async fn resolve_session_prefix(&self, recipient_did_prefix: &str) -> crate::error::Result<Option<String>> {
        for pair in self.reverse.iter() {
            if pair.key().starts_with(recipient_did_prefix) {
                return Ok(Some(pair.value().clone()));
            }
        }
        Ok(None)
    }
}

// ── In-memory DeviceTokenStore ─────────────────────────────────────────

#[derive(Debug, Default)]
pub struct InMemoryDeviceTokenStore {
    /// user_did -> fcm_device_token
    tokens: DashMap<String, String>,
    /// user_did -> push_channel ("fcm" or "websocket")
    push_channels: DashMap<String, String>,
}

impl InMemoryDeviceTokenStore {
    pub fn new() -> Self {
        Self {
            tokens: DashMap::new(),
            push_channels: DashMap::new(),
        }
    }
}

#[async_trait]
impl DeviceTokenStore for InMemoryDeviceTokenStore {
    async fn register_device_token(&self, user_did: &str, fcm_token: &str) -> crate::error::Result<()> {
        self.tokens.insert(user_did.to_string(), fcm_token.to_string());
        Ok(())
    }

    async fn get_device_token(&self, user_did: &str) -> crate::error::Result<Option<String>> {
        Ok(self.tokens.get(user_did).map(|v| v.value().clone()))
    }

    async fn set_push_channel(&self, user_did: &str, channel: &str) -> crate::error::Result<()> {
        self.push_channels.insert(user_did.to_string(), channel.to_string());
        Ok(())
    }

    async fn get_push_channel(&self, user_did: &str) -> crate::error::Result<String> {
        Ok(self
            .push_channels
            .get(user_did)
            .map(|v| v.value().clone())
            .unwrap_or_else(|| "fcm".to_string()))
    }
}

// ── In-memory AgentBindingStore ─────────────────────────────────────────

#[derive(Debug, Default)]
pub struct InMemoryAgentBindingStore {
    /// agent_did -> user_did
    agent_to_user: DashMap<String, String>,
    /// user_did -> set of agent_dids
    user_to_agents: DashMap<String, HashSet<String>>,
}

impl InMemoryAgentBindingStore {
    pub fn new() -> Self {
        Self {
            agent_to_user: DashMap::new(),
            user_to_agents: DashMap::new(),
        }
    }
}

#[async_trait]
impl AgentBindingStore for InMemoryAgentBindingStore {
    async fn bind(&self, agent_did: &str, user_did: &str) -> crate::error::Result<()> {
        self.agent_to_user
            .insert(agent_did.to_string(), user_did.to_string());
        self.user_to_agents
            .entry(user_did.to_string())
            .or_default()
            .insert(agent_did.to_string());
        Ok(())
    }

    async fn get_user_for_agent(&self, agent_did: &str) -> crate::error::Result<Option<String>> {
        Ok(self
            .agent_to_user
            .get(agent_did)
            .map(|v| v.value().clone()))
    }

    async fn get_agents_for_user(&self, user_did: &str) -> crate::error::Result<Vec<String>> {
        Ok(self
            .user_to_agents
            .get(user_did)
            .map(|agents| agents.value().iter().cloned().collect())
            .unwrap_or_default())
    }
}

// ── Shared state type aliases ───────────────────────────────────────────

pub type SharedMessageStore = Arc<dyn MessageStore>;
pub type SharedKeylistStore = Arc<dyn KeylistStore>;
pub type SharedDeviceTokenStore = Arc<dyn DeviceTokenStore>;
pub type SharedAgentBindingStore = Arc<dyn AgentBindingStore>;

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_device_token_store_basic() {
        let store = InMemoryDeviceTokenStore::new();

        // Initially no token
        assert!(store.get_device_token("user1").await.unwrap().is_none());

        // Register token
        store
            .register_device_token("user1", "fcm_token_123")
            .await
            .unwrap();
        assert_eq!(
            store.get_device_token("user1").await.unwrap(),
            Some("fcm_token_123".to_string())
        );
    }

    #[tokio::test]
    async fn test_push_channel_default_is_fcm() {
        let store = InMemoryDeviceTokenStore::new();

        // Default push channel is "fcm"
        assert_eq!(
            store.get_push_channel("user1").await.unwrap(),
            "fcm".to_string()
        );
    }

    #[tokio::test]
    async fn test_push_channel_set_and_get_websocket() {
        let store = InMemoryDeviceTokenStore::new();

        store
            .set_push_channel("user1", "websocket")
            .await
            .unwrap();
        assert_eq!(
            store.get_push_channel("user1").await.unwrap(),
            "websocket".to_string()
        );
    }

    #[tokio::test]
    async fn test_push_channel_set_and_get_fcm() {
        let store = InMemoryDeviceTokenStore::new();

        store.set_push_channel("user1", "fcm").await.unwrap();
        assert_eq!(
            store.get_push_channel("user1").await.unwrap(),
            "fcm".to_string()
        );
    }

    #[tokio::test]
    async fn test_push_channel_different_users() {
        let store = InMemoryDeviceTokenStore::new();

        store
            .set_push_channel("user1", "websocket")
            .await
            .unwrap();
        store.set_push_channel("user2", "fcm").await.unwrap();

        assert_eq!(
            store.get_push_channel("user1").await.unwrap(),
            "websocket".to_string()
        );
        assert_eq!(
            store.get_push_channel("user2").await.unwrap(),
            "fcm".to_string()
        );
        // user3 has no channel set, defaults to fcm
        assert_eq!(
            store.get_push_channel("user3").await.unwrap(),
            "fcm".to_string()
        );
    }

    #[tokio::test]
    async fn test_push_channel_overwrite() {
        let store = InMemoryDeviceTokenStore::new();

        store
            .set_push_channel("user1", "fcm")
            .await
            .unwrap();
        assert_eq!(
            store.get_push_channel("user1").await.unwrap(),
            "fcm".to_string()
        );

        // Overwrite to websocket
        store
            .set_push_channel("user1", "websocket")
            .await
            .unwrap();
        assert_eq!(
            store.get_push_channel("user1").await.unwrap(),
            "websocket".to_string()
        );
    }
}
