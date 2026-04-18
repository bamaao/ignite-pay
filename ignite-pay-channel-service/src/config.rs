use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub solana: SolanaConfig,
    pub channel: ChannelConfig,
    pub compliance: Option<ComplianceConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SolanaConfig {
    pub rpc_url: String,
    pub channel_program_id: String,
    pub keypair_path: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ChannelConfig {
    pub default_tree_depth: u32,
    pub default_challenge_duration: u64,
    pub default_min_challenge_delay: u64,
    pub default_settle_window: u64,
    pub auto_close_offset: u64,
    pub db_path: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ComplianceConfig {
    pub spending_threshold: u64,
    pub per_channel_limit: u64,
    pub window_slots: u64,
    pub travel_rule_threshold: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum Role {
    User,
    Provider,
    Hub,
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
