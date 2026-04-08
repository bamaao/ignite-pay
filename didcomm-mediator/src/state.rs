use std::sync::Arc;

use crate::config::Config;
use crate::did::resolver::MediatorDidAgent;
use crate::session::manager::SessionManager;
use crate::storage::in_memory::{InMemoryKeylistStore, InMemoryMessageStore, SharedKeylistStore, SharedMessageStore};

/// Shared application state accessible to all handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub did_agent: MediatorDidAgent,
    pub sessions: Arc<SessionManager>,
    pub message_store: SharedMessageStore,
    pub keylist_store: SharedKeylistStore,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let did_agent = MediatorDidAgent::new(config.mediator.did.clone());
        let sessions = Arc::new(SessionManager::new());
        let message_store = Arc::new(InMemoryMessageStore::new(config.mediator.max_queued_messages));
        let keylist_store = Arc::new(InMemoryKeylistStore::default());

        Self {
            config,
            did_agent,
            sessions,
            message_store,
            keylist_store,
        }
    }
}
