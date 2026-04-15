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
    /// Processed message IDs for replay protection: msg_id -> expiry timestamp
    pub seen_message_ids: Arc<dashmap::DashMap<String, i64>>,
}

impl RouterState {
    pub fn new(config: Config) -> Result<Self, crate::error::RouterError> {
        let did_agent = RouterDidAgent::new(config.router.did.clone());
        let sessions = Arc::new(SessionManager::new());

        // Create stores: sled-backed if storage.path is configured, in-memory otherwise
        let (message_store, keylist_store, device_token_store, agent_binding_store) =
            if let Some(ref path) = config.storage.path {
                tracing::info!("Persistent storage enabled: {}", path);
                let db = Arc::new(
                    sled::open(path).map_err(|e| crate::error::RouterError::Storage(e.to_string()))?,
                );
                (
                    Arc::new(crate::storage::sled_store::SledMessageStore::new(
                        db.clone(),
                        config.router.max_queued_messages,
                        config.router.max_message_age_seconds,
                    )) as SharedMessageStore,
                    Arc::new(crate::storage::sled_store::SledKeylistStore::new(db.clone()))
                        as SharedKeylistStore,
                    Arc::new(crate::storage::sled_store::SledDeviceTokenStore::new(db.clone()))
                        as SharedDeviceTokenStore,
                    Arc::new(crate::storage::sled_store::SledAgentBindingStore::new(db))
                        as SharedAgentBindingStore,
                )
            } else {
                tracing::warn!("No storage.path configured — using in-memory stores. All data will be lost on restart.");
                (
                    Arc::new(InMemoryMessageStore::with_max_age(
                        config.router.max_queued_messages,
                        config.router.max_message_age_seconds,
                    ))
                        as SharedMessageStore,
                    Arc::new(InMemoryKeylistStore::default()) as SharedKeylistStore,
                    Arc::new(InMemoryDeviceTokenStore::new()) as SharedDeviceTokenStore,
                    Arc::new(InMemoryAgentBindingStore::new()) as SharedAgentBindingStore,
                )
            };

        // Create notification sender: real FCM if configured, no-op otherwise
        let notification_sender: Arc<dyn NotificationSender> =
            if let Some(ref sa_path) = config.fcm.service_account_json {
                match crate::notification::fcm::FcmSender::from_service_account_file(
                    sa_path,
                    config.fcm.project_id.clone(),
                ) {
                    Ok(sender) => {
                        tracing::info!("FCM enabled: service account from {}", sa_path);
                        Arc::new(sender)
                    }
                    Err(e) => {
                        tracing::error!("Failed to init FCM sender from {}: {}. Falling back to no-op.", sa_path, e);
                        Arc::new(crate::notification::NoopNotificationSender)
                    }
                }
            } else {
                tracing::info!("FCM not configured (no service_account_json). Push notifications disabled.");
                Arc::new(crate::notification::NoopNotificationSender)
            };

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
            seen_message_ids: Arc::new(dashmap::DashMap::new()),
        })
    }
}
