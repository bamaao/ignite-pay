use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::Arc;

use super::{AgentBindingStore, DeviceTokenStore, KeylistStore, MessageStore, QueuedMessage};

// ── Sled-backed MessageStore ──────────────────────────────────────────

pub struct SledMessageStore {
    db: Arc<sled::Db>,
    max_queued: usize,
}

impl SledMessageStore {
    pub fn new(db: Arc<sled::Db>, max_queued: usize) -> Self {
        Self { db, max_queued }
    }

    fn queue_tree(&self, recipient_did: &str) -> sled::Result<sled::Tree> {
        self.db.open_tree(format!("msg:{}", recipient_did))
    }
}

#[async_trait]
impl MessageStore for SledMessageStore {
    async fn enqueue(&self, recipient_did: &str, msg: QueuedMessage) -> crate::error::Result<()> {
        let tree = self.queue_tree(recipient_did)?;
        let data = serde_json::to_vec(&msg)?;
        tree.insert(msg.id.as_bytes(), data.as_slice())?;

        // Enforce max capacity by removing oldest
        while tree.len() > self.max_queued {
            if let Some(oldest) = tree.first()? {
                tree.remove(&oldest.0)?;
            } else {
                break;
            }
        }
        Ok(())
    }

    async fn dequeue_batch(
        &self,
        recipient_did: &str,
        limit: usize,
    ) -> crate::error::Result<Vec<QueuedMessage>> {
        let tree = self.queue_tree(recipient_did)?;
        let mut messages = Vec::new();
        let mut keys_to_remove = Vec::new();

        for item in tree.iter().take(limit) {
            let (k, v) = item?;
            if let Ok(msg) = serde_json::from_slice::<QueuedMessage>(&v) {
                messages.push(msg);
                keys_to_remove.push(k);
            }
        }

        for k in keys_to_remove {
            tree.remove(&k)?;
        }

        Ok(messages)
    }

    async fn count(&self, recipient_did: &str) -> crate::error::Result<usize> {
        let tree = self.queue_tree(recipient_did)?;
        Ok(tree.len())
    }

    async fn remove(&self, msg_id: &str) -> crate::error::Result<()> {
        // Search across all message trees
        for tree_name in self.db.tree_names() {
            let tree = self.db.open_tree(&tree_name)?;
            if tree.remove(msg_id.as_bytes())?.is_some() {
                break;
            }
        }
        Ok(())
    }

    async fn store_for_user(&self, recipient_did: &str, msg: QueuedMessage) -> crate::error::Result<()> {
        // Same as enqueue — persists until explicitly removed
        self.enqueue(recipient_did, msg).await
    }

    async fn get_message(&self, recipient_did: &str, msg_id: &str) -> crate::error::Result<Option<QueuedMessage>> {
        let tree = self.queue_tree(recipient_did)?;
        match tree.get(msg_id.as_bytes())? {
            Some(data) => Ok(Some(serde_json::from_slice(&data)?)),
            None => Ok(None),
        }
    }

    async fn list_messages(
        &self,
        recipient_did: &str,
        after_id: Option<&str>,
        limit: usize,
    ) -> crate::error::Result<Vec<QueuedMessage>> {
        let tree = self.queue_tree(recipient_did)?;
        let mut messages = Vec::new();
        let mut found_start = after_id.is_none();

        for item in tree.iter() {
            let (_, v) = item?;
            let msg: QueuedMessage = serde_json::from_slice(&v)?;

            if !found_start {
                if msg.id == after_id.unwrap_or("") {
                    found_start = true;
                }
                continue;
            }

            messages.push(msg);
            if messages.len() >= limit {
                break;
            }
        }

        Ok(messages)
    }
}

// ── Sled-backed KeylistStore ──────────────────────────────────────────

pub struct SledKeylistStore {
    db: Arc<sled::Db>,
}

impl SledKeylistStore {
    pub fn new(db: Arc<sled::Db>) -> Self {
        Self { db }
    }

    fn keylist_tree(&self) -> sled::Result<sled::Tree> {
        self.db.open_tree("keylist")
    }

    fn reverse_tree(&self) -> sled::Result<sled::Tree> {
        self.db.open_tree("keylist_reverse")
    }
}

#[async_trait]
impl KeylistStore for SledKeylistStore {
    async fn add_key(&self, session_did: &str, recipient_did: &str) -> crate::error::Result<()> {
        let keylist = self.keylist_tree()?;
        let reverse = self.reverse_tree()?;

        // Add to forward map: session_did -> set of recipient_dids
        let key = format!("{}:{}", session_did, recipient_did);
        keylist.insert(key.as_bytes(), &b"1"[..])?;

        // Add reverse map: recipient_did -> session_did
        reverse.insert(recipient_did.as_bytes(), session_did.as_bytes())?;
        Ok(())
    }

    async fn remove_key(&self, session_did: &str, recipient_did: &str) -> crate::error::Result<()> {
        let keylist = self.keylist_tree()?;
        let reverse = self.reverse_tree()?;

        let key = format!("{}:{}", session_did, recipient_did);
        keylist.remove(key.as_bytes())?;
        reverse.remove(recipient_did.as_bytes())?;
        Ok(())
    }

    async fn list_keys(&self, session_did: &str) -> crate::error::Result<HashSet<String>> {
        let keylist = self.keylist_tree()?;
        let prefix = format!("{}:", session_did);
        let mut keys = HashSet::new();

        for item in keylist.scan_prefix(prefix.as_bytes()) {
            let (k, _) = item?;
            let key_str = String::from_utf8_lossy(&k);
            if let Some(recipient) = key_str.split(':').nth(1) {
                keys.insert(recipient.to_string());
            }
        }

        Ok(keys)
    }

    async fn resolve_session(&self, recipient_did: &str) -> crate::error::Result<Option<String>> {
        let reverse = self.reverse_tree()?;
        match reverse.get(recipient_did.as_bytes())? {
            Some(v) => Ok(Some(
                String::from_utf8(v.to_vec()).map_err(|e| crate::error::RouterError::Storage(e.to_string()))?,
            )),
            None => Ok(None),
        }
    }
}

// ── Sled-backed DeviceTokenStore ─────────────────────────────────────

pub struct SledDeviceTokenStore {
    db: Arc<sled::Db>,
}

impl SledDeviceTokenStore {
    pub fn new(db: Arc<sled::Db>) -> Self {
        Self { db }
    }

    fn token_tree(&self) -> sled::Result<sled::Tree> {
        self.db.open_tree("device_tokens")
    }

    fn channel_tree(&self) -> sled::Result<sled::Tree> {
        self.db.open_tree("push_channels")
    }
}

#[async_trait]
impl DeviceTokenStore for SledDeviceTokenStore {
    async fn register_device_token(&self, user_did: &str, fcm_token: &str) -> crate::error::Result<()> {
        let tree = self.token_tree()?;
        tree.insert(user_did.as_bytes(), fcm_token.as_bytes())?;
        Ok(())
    }

    async fn get_device_token(&self, user_did: &str) -> crate::error::Result<Option<String>> {
        let tree = self.token_tree()?;
        match tree.get(user_did.as_bytes())? {
            Some(v) => Ok(Some(
                String::from_utf8(v.to_vec()).map_err(|e| crate::error::RouterError::Storage(e.to_string()))?,
            )),
            None => Ok(None),
        }
    }

    async fn set_push_channel(&self, user_did: &str, channel: &str) -> crate::error::Result<()> {
        let tree = self.channel_tree()?;
        tree.insert(user_did.as_bytes(), channel.as_bytes())?;
        Ok(())
    }

    async fn get_push_channel(&self, user_did: &str) -> crate::error::Result<String> {
        let tree = self.channel_tree()?;
        match tree.get(user_did.as_bytes())? {
            Some(v) => Ok(String::from_utf8(v.to_vec())
                .map_err(|e| crate::error::RouterError::Storage(e.to_string()))?),
            None => Ok("fcm".to_string()),
        }
    }
}

// ── Sled-backed AgentBindingStore ────────────────────────────────────

pub struct SledAgentBindingStore {
    db: Arc<sled::Db>,
}

impl SledAgentBindingStore {
    pub fn new(db: Arc<sled::Db>) -> Self {
        Self { db }
    }

    fn agent_to_user_tree(&self) -> sled::Result<sled::Tree> {
        self.db.open_tree("agent_to_user")
    }

    fn user_to_agents_tree(&self) -> sled::Result<sled::Tree> {
        self.db.open_tree("user_to_agents")
    }
}

#[async_trait]
impl AgentBindingStore for SledAgentBindingStore {
    async fn bind(&self, agent_did: &str, user_did: &str) -> crate::error::Result<()> {
        let a2u = self.agent_to_user_tree()?;
        let u2a = self.user_to_agents_tree()?;

        // Forward: agent -> user
        a2u.insert(agent_did.as_bytes(), user_did.as_bytes())?;

        // Reverse: user -> agents (composite key)
        let rkey = format!("{}:{}", user_did, agent_did);
        u2a.insert(rkey.as_bytes(), &b"1"[..])?;
        Ok(())
    }

    async fn get_user_for_agent(&self, agent_did: &str) -> crate::error::Result<Option<String>> {
        let a2u = self.agent_to_user_tree()?;
        match a2u.get(agent_did.as_bytes())? {
            Some(v) => Ok(Some(
                String::from_utf8(v.to_vec()).map_err(|e| crate::error::RouterError::Storage(e.to_string()))?,
            )),
            None => Ok(None),
        }
    }

    async fn get_agents_for_user(&self, user_did: &str) -> crate::error::Result<Vec<String>> {
        let u2a = self.user_to_agents_tree()?;
        let prefix = format!("{}:", user_did);
        let mut agents = Vec::new();

        for item in u2a.scan_prefix(prefix.as_bytes()) {
            let (k, _) = item?;
            let key_str = String::from_utf8_lossy(&k);
            if let Some(agent) = key_str.split(':').nth(1) {
                agents.push(agent.to_string());
            }
        }

        Ok(agents)
    }
}
