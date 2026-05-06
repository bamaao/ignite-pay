use std::sync::Arc;

use crate::config::Config;
use crate::did::ignite_store::IgniteDidStore;
use ignite_pay_solana::compression::DidService;
use ignite_pay_solana::types::MerchantDidAccount;
#[cfg(feature = "zk-compression")]
use light_client::rpc::{LightClient, LightClientConfig, Rpc};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::Signer;

/// Encode an Ed25519 public key as a `did:ignite:z...` DID.
/// Mirrors the private `encode_did_ignite` in ignite-pay-core/src/identity.rs.
fn encode_did_ignite(pub_key: &[u8; 32]) -> String {
    let mut prefixed = vec![0xed, 0x01];
    prefixed.extend_from_slice(pub_key);
    let encoded = bs58::encode(&prefixed).into_string();
    format!("did:ignite:z{}", encoded)
}

/// Shared application state for the DID registry service.
#[derive(Clone)]
pub struct RegistryState {
    pub config: Config,
    pub did_service: Arc<DidService>,
    #[cfg(feature = "zk-compression")]
    pub light_rpc: Arc<tokio::sync::Mutex<LightClient>>,
    pub did_store: Arc<IgniteDidStore>,
    pub db: Arc<sled::Db>,
    pub payer: Arc<solana_sdk::signature::Keypair>,
    /// Issued nonces for replay protection: nonce -> expiry timestamp
    pub nonces: Arc<dashmap::DashMap<String, i64>>,
    /// Platform Ed25519 signing key for issuing VCs.
    pub platform_signing_key: Arc<ed25519_dalek::SigningKey>,
    /// Platform DID derived from the signing key's public key.
    pub platform_did: String,
}

impl RegistryState {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let did_service = DidService::new(
            &config.solana.rpc_url,
            &config.solana.did_program_id,
        )?;

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

        // Load platform signing key from file or generate ephemeral for dev
        let platform_signing_key = if config.auth.platform_signing_key_path.is_empty() {
            tracing::warn!("No platform signing key path configured, using ephemeral key (dev only)");
            ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]) // deterministic for dev
        } else {
            let key_bytes = std::fs::read(&config.auth.platform_signing_key_path)?;
            if key_bytes.len() != 32 {
                anyhow::bail!("Platform signing key file must contain exactly 32 bytes, got {}", key_bytes.len());
            }
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(&key_bytes);
            ed25519_dalek::SigningKey::from_bytes(&bytes)
        };
        let verifying_key = platform_signing_key.verifying_key();
        let platform_did = encode_did_ignite(verifying_key.as_bytes());
        tracing::info!("Platform DID: {}", platform_did);

        // Initialize LightClient only in ZK Compression mode
        #[cfg(feature = "zk-compression")]
        let light_rpc = {
            let photon_url = config.light.photon_url.clone();
            let rpc_url = config.solana.rpc_url.clone();
            let light = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    let light_config = LightClientConfig::new(
                        rpc_url,
                        Some(photon_url),
                    );
                    LightClient::new(light_config).await
                })
            })?;
            Arc::new(tokio::sync::Mutex::new(light))
        };

        Ok(Self {
            config,
            did_service: Arc::new(did_service),
            #[cfg(feature = "zk-compression")]
            light_rpc,
            did_store: Arc::new(did_store),
            db: Arc::new(db),
            payer: Arc::new(payer),
            nonces: Arc::new(dashmap::DashMap::new()),
            platform_signing_key: Arc::new(platform_signing_key),
            platform_did,
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

    /// Sign (credential_subject_pk || vc_hash) with the platform Ed25519 key.
    /// Returns 64-byte Ed25519 signature.
    pub fn sign_vc_binding(&self, credential_subject_pk: &Pubkey, vc_hash: &[u8; 32]) -> [u8; 64] {
        use ed25519_dalek::Signer;
        let mut message = Vec::with_capacity(64);
        message.extend_from_slice(credential_subject_pk.as_ref());
        message.extend_from_slice(vc_hash);
        let signature: ed25519_dalek::Signature = self.platform_signing_key.sign(&message);
        signature.to_bytes()
    }

    /// Derive the PlatformConfig PDA address for the DID program.
    pub fn platform_config_address(&self) -> Pubkey {
        Pubkey::find_program_address(
            &[b"platform-config"],
            &self.did_service.did_program_id,
        ).0
    }

    /// Derive the RevokedVc PDA address for a given vc_hash.
    pub fn revoked_vc_address(&self, vc_hash: &[u8; 32]) -> Pubkey {
        Pubkey::find_program_address(
            &[b"revoked-vc", vc_hash],
            &self.did_service.did_program_id,
        ).0
    }
}
