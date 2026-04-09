use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub mediator: MediatorConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MediatorConfig {
    pub did: String,
    pub max_queued_messages: usize,
    pub max_message_age_seconds: u64,
    #[serde(default)]
    pub known_peers: Vec<KnownPeerConfig>,
}

/// A pre-configured peer whose keys are known at startup.
#[derive(Debug, Deserialize, Clone)]
pub struct KnownPeerConfig {
    pub did: String,
    pub key_agreement_kid: String,
    pub key_agreement_public_base64: String,
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
