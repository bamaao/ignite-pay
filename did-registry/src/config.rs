use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub solana: SolanaConfig,
    #[cfg(feature = "zk-compression")]
    pub light: LightConfig,
    pub auth: AuthConfig,
    pub fees: FeesConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SolanaConfig {
    pub rpc_url: String,
    /// DID program ID (ignite-pay-did-program)
    pub did_program_id: String,
    pub payer_keypair_path: String,
}

#[cfg(feature = "zk-compression")]
#[derive(Debug, Deserialize, Clone)]
pub struct LightConfig {
    /// Photon RPC URL for ZK Compression (e.g., https://photon.helius.com?api-key=KEY)
    pub photon_url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub platform_public_key: String,
    /// Path to a file containing 32 raw bytes of an Ed25519 private key
    /// used for signing VCs. If empty, an ephemeral key is generated (dev only).
    pub platform_signing_key_path: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FeesConfig {
    pub register_fee_lamports: u64,
    pub update_vc_fee_lamports: u64,
    pub rotate_key_fee_lamports: u64,
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
