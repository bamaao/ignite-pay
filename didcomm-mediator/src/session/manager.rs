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
                .map_err(|e| crate::error::MediatorError::SessionNotFound(e.to_string())),
            None => Err(crate::error::MediatorError::SessionNotFound(did.to_string())),
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
