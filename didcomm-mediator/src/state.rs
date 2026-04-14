use std::sync::Arc;

use crate::config::Config;
use crate::did::ignite_resolver::IgniteDidRegistry;
use crate::did::resolver::MediatorDidAgent;
use crate::notification::NotificationSender;
use crate::session::manager::SessionManager;
use crate::storage::in_memory::{
    InMemoryAgentBindingStore, InMemoryDeviceTokenStore, InMemoryKeylistStore,
    InMemoryMessageStore, SharedAgentBindingStore, SharedDeviceTokenStore, SharedKeylistStore,
    SharedMessageStore,
};

/// Shared application state accessible to all handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub did_agent: MediatorDidAgent,
    pub sessions: Arc<SessionManager>,
    pub message_store: SharedMessageStore,
    pub keylist_store: SharedKeylistStore,
    pub ignite_registry: Arc<IgniteDidRegistry>,
    pub device_token_store: SharedDeviceTokenStore,
    pub notification_sender: Arc<dyn NotificationSender>,
    pub agent_binding_store: SharedAgentBindingStore,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let did_agent = MediatorDidAgent::new(config.mediator.did.clone());
        let sessions = Arc::new(SessionManager::new());
        let message_store = Arc::new(InMemoryMessageStore::new(config.mediator.max_queued_messages));
        let keylist_store = Arc::new(InMemoryKeylistStore::default());
        let ignite_registry = Arc::new(IgniteDidRegistry::new());
        let device_token_store = Arc::new(InMemoryDeviceTokenStore::new());
        let notification_sender = Arc::new(crate::notification::NoopNotificationSender);
        let agent_binding_store = Arc::new(InMemoryAgentBindingStore::new());

        Self {
            config,
            did_agent,
            sessions,
            message_store,
            keylist_store,
            ignite_registry,
            device_token_store,
            notification_sender,
            agent_binding_store,
        }
    }
}
