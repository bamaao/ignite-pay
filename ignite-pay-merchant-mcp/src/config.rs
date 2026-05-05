use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub mcp: McpConfig,
    pub merchant: MerchantConfig,
    pub mediator: MediatorConfig,
    pub storage: StorageConfig,
    pub solana: SolanaConfig,
    pub hub: HubConfig,
    #[serde(default)]
    pub magicblock: MagicBlockConfig,
    #[serde(default)]
    pub did_registry: DidRegistryConfig,
}

#[derive(Debug, Deserialize, Default)]
pub struct McpConfig {
    #[serde(default)]
    pub sse_port: u16,
}

#[derive(Debug, Deserialize)]
pub struct MerchantConfig {
    #[serde(default)]
    pub did: String,
    pub hub_endpoint: String,
    pub hub_ws_url: String,
    #[serde(default)]
    pub wallet: String,
    #[serde(default = "default_accept_tokens")]
    pub accept_tokens: Vec<String>,
}

fn default_accept_tokens() -> Vec<String> {
    vec!["USDC".to_string()]
}

#[derive(Debug, Deserialize)]
pub struct MediatorConfig {
    pub ws_url: String,
}

#[derive(Debug, Deserialize)]
pub struct StorageConfig {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct SolanaConfig {
    #[serde(default = "default_rpc_url")]
    pub rpc_url: String,
    #[serde(default)]
    pub program_id: String,
}

#[derive(Debug, Deserialize)]
pub struct HubConfig {
    #[serde(default)]
    pub token_mint: String,
    #[serde(default)]
    pub provider_pubkey: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct MagicBlockConfig {
    #[serde(default = "default_rpc_url")]
    pub rpc_url: String,
    #[serde(default = "default_mb_program_id")]
    pub program_id: String,
}

fn default_mb_program_id() -> String {
    "6pFXAg1oiV61wVvaJvMHqYdGMe2fscDwmN9UBUSvNuU3".to_string()
}

fn default_rpc_url() -> String {
    "https://api.devnet.solana.com".to_string()
}

#[derive(Debug, Deserialize, Default)]
pub struct DidRegistryConfig {
    #[serde(default)]
    pub url: String,
}

pub fn load_config() -> anyhow::Result<Config> {
    let config_path =
        std::env::var("IGNITE_MERCHANT_CONFIG").unwrap_or_else(|_| "config.toml".to_string());
    let content = std::fs::read_to_string(&config_path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}
