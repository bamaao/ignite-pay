use axum::extract::ws::Message;
use dashmap::DashMap;
use tokio::sync::mpsc;

/// Handle for sending text to a connected WebSocket peer.
pub type WsSender = mpsc::UnboundedSender<Message>;

/// Manages live WebSocket sessions: DID -> channel sender.
/// Thread-safe via DashMap.
#[derive(Debug, Default)]
pub struct SessionManager {
    sessions: DashMap<String, WsSender>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
        }
    }

    /// Register a session. If the DID already had a session, the old one is replaced.
    pub fn register(&self, did: String, sender: WsSender) {
        self.sessions.insert(did, sender);
    }

    /// Unregister a session when the WebSocket disconnects.
    pub fn unregister(&self, did: &str) {
        self.sessions.remove(did);
    }

    /// Try to send a text message to a connected peer.
    /// Returns Ok(()) if sent, Err if not connected or channel closed.
    pub fn send_to(&self, did: &str, text: &str) -> crate::error::Result<()> {
        match self.sessions.get(did) {
            Some(sender) => sender
                .send(Message::Text(text.into()))
                .map_err(|e| crate::error::RouterError::SessionNotFound(e.to_string())),
            None => Err(crate::error::RouterError::SessionNotFound(did.to_string())),
        }
    }

    /// Check if a DID is currently connected.
    pub fn is_online(&self, did: &str) -> bool {
        self.sessions.contains_key(did)
    }

    /// List all connected DIDs.
    pub fn connected_dids(&self) -> Vec<String> {
        self.sessions.iter().map(|e| e.key().clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_is_online() {
        let mgr = SessionManager::new();
        let (tx, _rx) = mpsc::unbounded_channel();

        assert!(!mgr.is_online("did:test:agent1"));

        mgr.register("did:test:agent1".to_string(), tx);
        assert!(mgr.is_online("did:test:agent1"));
    }

    #[test]
    fn test_unregister() {
        let mgr = SessionManager::new();
        let (tx, _rx) = mpsc::unbounded_channel();

        mgr.register("did:test:agent1".to_string(), tx);
        assert!(mgr.is_online("did:test:agent1"));

        mgr.unregister("did:test:agent1");
        assert!(!mgr.is_online("did:test:agent1"));
    }

    #[test]
    fn test_send_to_online() {
        let mgr = SessionManager::new();
        let (tx, mut rx) = mpsc::unbounded_channel();

        mgr.register("did:test:agent1".to_string(), tx);

        let result = mgr.send_to("did:test:agent1", "hello");
        assert!(result.is_ok());

        let msg = rx.try_recv().unwrap();
        match msg {
            Message::Text(text) => assert_eq!(text, "hello"),
            _ => panic!("Expected Text message"),
        }
    }

    #[test]
    fn test_send_to_offline_returns_error() {
        let mgr = SessionManager::new();

        let result = mgr.send_to("did:test:notexist", "hello");
        assert!(result.is_err());
    }

    #[test]
    fn test_connected_dids() {
        let mgr = SessionManager::new();
        let (tx1, _rx1) = mpsc::unbounded_channel();
        let (tx2, _rx2) = mpsc::unbounded_channel();

        mgr.register("did:test:a".to_string(), tx1);
        mgr.register("did:test:b".to_string(), tx2);

        let mut dids = mgr.connected_dids();
        dids.sort();
        assert_eq!(dids, vec!["did:test:a", "did:test:b"]);
    }
}
