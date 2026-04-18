use std::sync::Arc;
use tokio::sync::mpsc;

use dashmap::DashMap;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::Signer;

use ignite_pay_state_channel::channel::ChannelManager;
use ignite_pay_state_channel::compliance::ComplianceManager;
use ignite_pay_state_channel::hub::HubManager;
use ignite_pay_state_channel::multihop::MultiHopManager;
use ignite_pay_state_channel::routing::RouteService;

use crate::config::{Config, Role};
use crate::error::ChannelServiceError;
use crate::ws::protocol::WsMessage;

/// Shared application state for the channel service.
#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub role: Role,
    pub db: Arc<sled::Db>,
    pub channel_manager: Arc<tokio::sync::Mutex<ChannelManager>>,
    /// Hub-only managers
    pub hub_manager: Option<Arc<tokio::sync::Mutex<HubManager>>>,
    pub route_service: Option<Arc<tokio::sync::Mutex<RouteService>>>,
    pub multihop_manager: Option<Arc<tokio::sync::Mutex<MultiHopManager>>>,
    /// Optional compliance
    pub compliance_manager: Option<Arc<tokio::sync::Mutex<ComplianceManager>>>,
    /// Ed25519 keypair bytes (v1 format: 64 bytes = 32 secret + 32 public)
    pub keypair_bytes: Arc<[u8; 64]>,
    /// Solana RPC client
    pub rpc_client: Arc<RpcClient>,
    /// On-chain program ID
    pub program_id: Arc<Pubkey>,
    /// Connected WebSocket peers: pubkey_base58 -> sender
    pub ws_peers: Arc<DashMap<String, mpsc::Sender<WsMessage>>>,
}

impl AppState {
    pub fn new(config: Config, role: Role) -> anyhow::Result<Self> {
        let db = sled::open(&config.channel.db_path)?;
        let db = Arc::new(db);

        let channel_manager = ChannelManager::new((*db).clone())?;
        let channel_manager = Arc::new(tokio::sync::Mutex::new(channel_manager));

        // Load keypair from file
        let keypair_bytes = if config.solana.keypair_path.is_empty() {
            let kp = solana_sdk::signer::keypair::Keypair::new();
            let mut bytes = [0u8; 64];
            bytes.copy_from_slice(&kp.to_bytes());
            bytes
        } else {
            let raw = std::fs::read(&config.solana.keypair_path)?;
            let kp = solana_sdk::signer::keypair::Keypair::try_from(raw.as_slice())?;
            let mut bytes = [0u8; 64];
            bytes.copy_from_slice(&kp.to_bytes());
            bytes
        };

        // Derive ed25519_dalek v1 Keypair for state-channel operations
        let ed_kp = ed25519_dalek::Keypair::from_bytes(&keypair_bytes)
            .map_err(|e| anyhow::anyhow!("invalid keypair: {}", e))?;
        let pubkey_bs58 = bs58::encode(ed_kp.public.to_bytes()).into_string();

        tracing::info!(
            "Starting as {:?} with pubkey {}",
            role,
            pubkey_bs58
        );

        let rpc_client = Arc::new(RpcClient::new(&config.solana.rpc_url));

        let program_id = config.solana.channel_program_id.parse::<Pubkey>()
            .map_err(|e| anyhow::anyhow!("invalid channel_program_id: {}", e))?;

        // Hub-only managers
        let (hub_manager, route_service, multihop_manager) = if role == Role::Hub {
            let hub_mgr = HubManager::new((*db).clone())?;
            let route_svc = RouteService::new(hub_mgr);
            let hub_mgr = Arc::new(tokio::sync::Mutex::new(
                HubManager::new((*db).clone())?
            ));
            let multihop_mgr = Arc::new(tokio::sync::Mutex::new(
                MultiHopManager::new((*db).clone())?
            ));
            (Some(hub_mgr), Some(Arc::new(tokio::sync::Mutex::new(route_svc))), Some(multihop_mgr))
        } else {
            (None, None, None)
        };

        // Optional compliance
        let compliance_manager = match (&config.compliance, role) {
            (Some(_), Role::User) | (Some(_), Role::Hub) => {
                let mgr = ComplianceManager::new((*db).clone())?;
                Some(Arc::new(tokio::sync::Mutex::new(mgr)))
            }
            _ => None,
        };

        Ok(Self {
            config,
            role,
            db,
            channel_manager,
            hub_manager,
            route_service,
            multihop_manager,
            compliance_manager,
            keypair_bytes: Arc::new(keypair_bytes),
            rpc_client,
            program_id: Arc::new(program_id),
            ws_peers: Arc::new(DashMap::new()),
        })
    }

    /// Get our public key as a Solana Pubkey.
    pub fn pubkey(&self) -> Pubkey {
        let kp = solana_sdk::signer::keypair::Keypair::try_from(&*self.keypair_bytes as &[u8])
            .expect("keypair_bytes must be valid");
        kp.pubkey()
    }

    /// Reconstruct the ed25519_dalek v1 Keypair for state-channel signing.
    pub fn ed_keypair(&self) -> ed25519_dalek::Keypair {
        ed25519_dalek::Keypair::from_bytes(&*self.keypair_bytes)
            .expect("keypair_bytes must be valid")
    }

    /// Initialize compliance for a channel if compliance is configured.
    pub async fn init_compliance_for_channel(
        &self,
        channel_id: [u8; 32],
    ) -> Result<(), ChannelServiceError> {
        if let (Some(ref mgr), Some(ref cfg)) = (&self.compliance_manager, &self.config.compliance) {
            let limits = ignite_pay_state_channel::compliance::SpendingLimit {
                threshold: cfg.spending_threshold,
                per_channel: cfg.per_channel_limit,
                window_slots: cfg.window_slots,
            };
            let mgr = mgr.lock().await;
            mgr.init_channel_compliance(channel_id, limits)
                .map_err(ChannelServiceError::StateChannel)?;
        }
        Ok(())
    }
}
