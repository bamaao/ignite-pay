use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub router: RouterConfig,
    #[serde(default)]
    pub fcm: FcmConfig,
    #[serde(default)]
    pub storage: StorageConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RouterConfig {
    pub did: String,
    pub max_queued_messages: usize,
    pub max_message_age_seconds: u64,
    #[serde(default)]
    pub known_peers: Vec<KnownPeerConfig>,
    #[serde(default = "default_jwt_secret")]
    pub jwt_secret: String,
}

fn default_jwt_secret() -> String {
    use std::env;
    env::var("JWT_SECRET").unwrap_or_else(|_| {
        tracing::warn!("JWT_SECRET not set and not configured; using random secret. Set [router] jwt_secret in config or JWT_SECRET env var.");
        uuid::Uuid::new_v4().to_string()
    })
}

/// A pre-configured peer whose keys are known at startup.
#[derive(Debug, Deserialize, Clone)]
pub struct KnownPeerConfig {
    pub did: String,
    pub key_agreement_kid: String,
    pub key_agreement_public_base64: String,
}

/// FCM (Firebase Cloud Messaging) configuration.
/// When `service_account_json` is set, the router will send real FCM push notifications.
/// When absent, a no-op sender is used.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct FcmConfig {
    /// Path to a Firebase service account JSON key file.
    /// Download from Firebase Console > Project Settings > Service Accounts > Generate New Private Key.
    pub service_account_json: Option<String>,
    /// Firebase project ID (required if service_account_json is set).
    pub project_id: Option<String>,
}

/// Storage configuration.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct StorageConfig {
    /// Directory path for sled database files.
    /// If set, all stores use sled for persistence across restarts.
    /// If absent, in-memory stores are used (data lost on restart).
    pub path: Option<String>,
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }
}
