use std::sync::Arc;

use crate::config::Config;
use crate::did::resolver::RouterDidAgent;
use crate::notification::NotificationSender;
use crate::session::manager::SessionManager;
use crate::storage::in_memory::{
    InMemoryAgentBindingStore, InMemoryDeviceTokenStore, InMemoryKeylistStore,
    InMemoryMessageStore, SharedAgentBindingStore, SharedDeviceTokenStore, SharedKeylistStore,
    SharedMessageStore,
};

/// Shared application state accessible to all handlers.
#[derive(Clone)]
pub struct RouterState {
    pub config: Config,
    pub did_agent: RouterDidAgent,
    pub sessions: Arc<SessionManager>,
    pub message_store: SharedMessageStore,
    pub keylist_store: SharedKeylistStore,
    pub device_token_store: SharedDeviceTokenStore,
    pub notification_sender: Arc<dyn NotificationSender>,
    pub agent_binding_store: SharedAgentBindingStore,
    pub auth_challenges: Arc<dashmap::DashMap<String, i64>>, // nonce -> expiry timestamp
}

impl RouterState {
    pub fn new(config: Config) -> Result<Self, crate::error::RouterError> {
        let did_agent = RouterDidAgent::new(config.router.did.clone());
        let sessions = Arc::new(SessionManager::new());
        let message_store = Arc::new(InMemoryMessageStore::new(config.router.max_queued_messages));
        let keylist_store = Arc::new(InMemoryKeylistStore::default());
        let device_token_store = Arc::new(InMemoryDeviceTokenStore::new());
        let notification_sender = Arc::new(crate::notification::NoopNotificationSender);
        let agent_binding_store = Arc::new(InMemoryAgentBindingStore::new());

        Ok(Self {
            config,
            did_agent,
            sessions,
            message_store,
            keylist_store,
            device_token_store,
            notification_sender,
            agent_binding_store,
            auth_challenges: Arc::new(dashmap::DashMap::new()),
        })
    }
}
