use std::sync::Arc;

use crate::config::Config;
use crate::did::ignite_store::IgniteDidStore;
use ignite_pay_solana::compression::DidService;
use ignite_pay_solana::types::MerchantDidAccount;
use light_client::rpc::{LightClient, LightClientConfig, Rpc};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::Signer;

/// Shared application state for the DID registry service.
/// Uses ZK Compression via LightClient (Photon RPC) for reading compressed
/// accounts and obtaining validity proofs, and DidService for building
/// and sending on-chain transactions.
#[derive(Clone)]
pub struct RegistryState {
    pub config: Config,
    pub did_service: Arc<DidService>,
    pub light_rpc: Arc<tokio::sync::Mutex<LightClient>>,
    pub did_store: Arc<IgniteDidStore>,
    pub db: Arc<sled::Db>,
    pub payer: Arc<solana_sdk::signature::Keypair>,
    /// Issued nonces for replay protection: nonce -> expiry timestamp
    pub nonces: Arc<dashmap::DashMap<String, i64>>,
}

impl RegistryState {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let did_service = DidService::new(
            &config.solana.rpc_url,
            &config.solana.did_program_id,
        )?;

        // We need to create LightClient async, so we'll do it in a blocking context
        // or use a placeholder. Since LightClient::new is async, we'll spawn it.
        let photon_url = config.light.photon_url.clone();
        let rpc_url = config.solana.rpc_url.clone();

        let db = sled::open("./did_registry_data")?;

        let did_store = IgniteDidStore::new();

        // Load payer keypair from file or generate ephemeral one for dev
        let payer = if config.solana.payer_keypair_path.is_empty() {
            tracing::warn!("No payer keypair path configured, using ephemeral keypair (dev only)");
            solana_sdk::signature::Keypair::new()
        } else {
            let keypair_bytes = std::fs::read(&config.solana.payer_keypair_path)?;
            solana_sdk::signature::Keypair::try_from(keypair_bytes.as_slice())?
        };

        tracing::info!("Registry payer pubkey: {}", payer.pubkey());

        // Create LightClient synchronously (using tokio runtime handle)
        let light_rpc = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let light_config = LightClientConfig::new(
                    rpc_url,
                    Some(photon_url),
                );
                LightClient::new(light_config).await
            })
        })?;

        Ok(Self {
            config,
            did_service: Arc::new(did_service),
            light_rpc: Arc::new(tokio::sync::Mutex::new(light_rpc)),
            did_store: Arc::new(did_store),
            db: Arc::new(db),
            payer: Arc::new(payer),
            nonces: Arc::new(dashmap::DashMap::new()),
        })
    }

    /// Look up a cached merchant DID account from local sled cache.
    pub fn get_cached_merchant(&self, did_hash: &[u8; 32]) -> Option<MerchantDidAccount> {
        let store = crate::storage::sled_store::MerchantStore::new((*self.db).clone());
        if let Some(bytes) = store.get_merchant(did_hash) {
            if let Ok(did) = <MerchantDidAccount as borsh::BorshDeserialize>::deserialize(&mut bytes.as_slice()) {
                return Some(did);
            }
        }
        None
    }

    /// Cache a merchant DID account in sled.
    pub fn cache_merchant(&self, did_hash: &[u8; 32], did: &MerchantDidAccount) {
        let store = crate::storage::sled_store::MerchantStore::new((*self.db).clone());
        if let Ok(bytes) = borsh::to_vec(did) {
            let _ = store.save_merchant(did_hash, &bytes);
        }
    }

    /// Get the DID program ID.
    pub fn did_program_id(&self) -> Pubkey {
        self.did_service.did_program_id
    }
}
