use async_trait::async_trait;
use dashmap::DashMap;
use std::collections::HashSet;
use std::sync::Arc;

use super::{KeylistStore, MessageStore, QueuedMessage};

// ── In-memory MessageStore ──────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct InMemoryMessageStore {
    /// recipient_did -> Vec<QueuedMessage>
    queues: DashMap<String, Vec<QueuedMessage>>,
    max_queued: usize,
}

impl InMemoryMessageStore {
    pub fn new(max_queued: usize) -> Self {
        Self {
            queues: DashMap::new(),
            max_queued,
        }
    }
}

#[async_trait]
impl MessageStore for InMemoryMessageStore {
    async fn enqueue(&self, recipient_did: &str, msg: QueuedMessage) -> crate::error::Result<()> {
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
}

// ── Shared state type aliases ───────────────────────────────────────────

pub type SharedMessageStore = Arc<dyn MessageStore>;
pub type SharedKeylistStore = Arc<dyn KeylistStore>;
