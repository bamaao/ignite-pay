use std::sync::Arc;

use crate::config::Config;
use crate::did::ignite_store::IgniteDidStore;
use ignite_pay_solana::compression::CompressionService;
use ignite_pay_solana::indexer::IndexerClient;
use ignite_pay_solana::types::MerchantLeaf;
use solana_sdk::signer::Signer;

/// Shared application state for the DID registry service.
#[derive(Clone)]
pub struct RegistryState {
    pub config: Config,
    pub compression: Arc<CompressionService>,
    pub indexer: Arc<IndexerClient>,
    pub did_store: Arc<IgniteDidStore>,
    pub db: Arc<sled::Db>,
    pub payer: Arc<solana_sdk::signature::Keypair>,
    /// Issued nonces for replay protection: nonce -> expiry timestamp
    pub nonces: Arc<dashmap::DashMap<String, i64>>,
}

impl RegistryState {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let compression = CompressionService::new(
            &config.solana.rpc_url,
            &config.solana.tree_address,
            &config.solana.tree_authority,
        )?;

        let indexer = IndexerClient::new(&config.indexer.das_endpoint)?;

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

        Ok(Self {
            config,
            compression: Arc::new(compression),
            indexer: Arc::new(indexer),
            did_store: Arc::new(did_store),
            db: Arc::new(db),
            payer: Arc::new(payer),
            nonces: Arc::new(dashmap::DashMap::new()),
        })
    }

    /// Look up a merchant leaf from the local sled cache.
    pub fn get_cached_merchant(&self, did_hash: &[u8; 32]) -> Option<(u32, MerchantLeaf)> {
        let key = hex::encode(did_hash);
        if let Some(bytes) = self.db.get(format!("merchant:{}", key)).ok().flatten() {
            if let Ok((leaf_index, leaf)) = borsh::from_slice::<(u32, MerchantLeaf)>(&bytes) {
                return Some((leaf_index, leaf));
            }
        }
        None
    }

    /// Cache a merchant leaf in sled.
    pub fn cache_merchant(&self, did_hash: &[u8; 32], leaf_index: u32, leaf: &MerchantLeaf) {
        let key = format!("merchant:{}", hex::encode(did_hash));
        if let Ok(bytes) = borsh::to_vec(&(leaf_index, leaf)) {
            let _ = self.db.insert(key, bytes);
        }
    }
}
