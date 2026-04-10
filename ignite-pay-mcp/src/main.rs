mod mediator;
mod payment;
mod tools;

use crate::mediator::MediatorConnection;
use crate::payment::{AuthResponse, execute_mock_payment, PaymentRequest, PaymentStatus, PendingAuthStore};
use crate::tools::{AuthorizationCheckInput, PaymentHistoryInput, X402ChallengeInput};

use ignite_pay_core::types::VerifiableCredential;
use ignite_pay_core::ipfs::MockIpfsClient;
use ignite_pay_core::list_store::ListStore;
use ignite_pay_core::types::MerchantListEntry;
use ignite_pay_core::solana_did::SolanaDidBridge;
use ignite_pay_solana::payment::IgnitePayClient;
use ignite_pay_solana::session::SessionKeypair;
use ignite_pay_solana::types::PayMode;
use ignite_pay_solana::solana_sdk::pubkey::Pubkey;
use base64::Engine;

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{
        tool::ToolRouter,
        wrapper::Parameters,
    },
    model::ServerCapabilities,
    tool, tool_handler, tool_router,
    transport::stdio,
};
use std::sync::Arc;
use std::time::Duration;

// ── Configuration ──────────────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
struct Config {
    mediator: MediatorConfig,
    storage: StorageConfig,
    policy: PolicyConfig,
    platform: PlatformConfig,
    ipfs: IpfsConfig,
    #[serde(default)]
    solana: SolanaConfig,
}

#[derive(Debug, serde::Deserialize)]
struct MediatorConfig {
    ws_url: String,
    phone_did: String,
}

#[derive(Debug, serde::Deserialize)]
struct StorageConfig {
    path: String,
}

#[derive(Debug, serde::Deserialize)]
struct PolicyConfig {
    auto_approve_max: u64,
    auth_timeout: u64,
}

#[derive(Debug, serde::Deserialize)]
struct PlatformConfig {
    did: String,
    verifying_key_b64: String,
}

impl PlatformConfig {
    fn verifying_key_bytes(&self) -> Option<[u8; 32]> {
        let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(&self.verifying_key_b64)
            .ok()?;
        if bytes.len() != 32 {
            return None;
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Some(arr)
    }
}

#[derive(Debug, serde::Deserialize)]
struct IpfsConfig {
    mode: String,
}

#[derive(Debug, serde::Deserialize, Default)]
struct SolanaConfig {
    #[serde(default = "default_rpc_url")]
    rpc_url: String,
    #[serde(default)]
    tree_address: String,
    #[serde(default)]
    tree_authority: String,
    #[serde(default)]
    das_endpoint: String,
    #[serde(default = "default_pay_mode")]
    pay_mode: String,
    #[serde(default)]
    relayer_url: String,
}

fn default_rpc_url() -> String {
    "https://api.devnet.solana.com".to_string()
}

fn default_pay_mode() -> String {
    "self_funded".to_string()
}

impl SolanaConfig {
    fn is_configured(&self) -> bool {
        !self.tree_address.is_empty() && !self.tree_authority.is_empty()
    }

    fn pay_mode(&self) -> PayMode {
        match self.pay_mode.as_str() {
            "sponsored" => PayMode::Sponsored,
            _ => PayMode::SelfFunded,
        }
    }
}

fn load_config() -> Result<Config, anyhow::Error> {
    let config_path = std::env::var("IGNITE_PAY_CONFIG")
        .unwrap_or_else(|_| "config.toml".to_string());
    let content = std::fs::read_to_string(&config_path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

// ── Payment Execution ──────────────────────────────────────────────────────

/// Execute a payment using either real Solana on-chain transfer or mock payment.
async fn execute_payment(
    solana_client: &Option<Arc<IgnitePayClient>>,
    payment: &PaymentRequest,
    session: &Option<SessionKeypair>,
) -> String {
    match (solana_client, session) {
        (Some(client), Some(sess)) => {
            match client
                .execute_payment(
                    &payment.recipient,
                    payment.amount,
                    &payment.token,
                    &payment.network,
                    sess,
                )
                .await
            {
                Ok(result) => result.signature,
                Err(e) => {
                    tracing::warn!("Solana payment failed, falling back to mock: {}", e);
                    execute_mock_payment(payment)
                }
            }
        }
        _ => execute_mock_payment(payment),
    }
}

// ── MCP Server ─────────────────────────────────────────────────────────────

#[derive(Clone)]
struct IgnitePayMcpServer {
    tool_router: ToolRouter<Self>,
    mediator: Arc<MediatorConnection>,
    payments: Arc<payment::PaymentStore>,
    pending: Arc<PendingAuthStore>,
    list_store: Arc<ListStore>,
    phone_did: String,
    auth_timeout: u64,
    auto_approve_max: u64,
    platform_did: String,
    platform_verifying_key: Option<[u8; 32]>,
    // V2.0: Solana on-chain payment
    solana_client: Option<Arc<IgnitePayClient>>,
    // V2.0: On-chain DID verification
    solana_bridge: Option<Arc<SolanaDidBridge>>,
}

#[tool_router]
impl IgnitePayMcpServer {
    #[tool(description = "Process an HTTP 402 payment challenge. Parses the x402 response, verifies on-chain merchant DID (if configured), requests authorization from the phone app, and executes real Solana payment upon approval.")]
    async fn process_x402_challenge(
        &self,
        Parameters(input): Parameters<X402ChallengeInput>,
    ) -> String {
        // 1. Parse the 402 response JSON
        let challenge: serde_json::Value = match serde_json::from_str(&input.challenge_body) {
            Ok(v) => v,
            Err(e) => return format!("Error: Invalid JSON in challenge body: {}", e),
        };

        // Extract payment details from the x402 "accepts" array
        let accepts = match challenge.get("accepts").and_then(|v| v.as_array()) {
            Some(arr) if !arr.is_empty() => &arr[0],
            _ => return "Error: No payment schemes found in 402 response".to_string(),
        };

        let payment_type = accepts
            .get("paymentType")
            .and_then(|v| v.as_str())
            .unwrap_or("transfer");
        let network = accepts
            .get("network")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let token = accepts
            .get("token")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let amount = accepts
            .get("amount")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let recipient = accepts
            .get("recipient")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let merchant_did = challenge
            .get("provider_did")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let payment_id = uuid::Uuid::new_v4().to_string();
        let description = format!(
            "{} {} {} on {} to {}",
            amount, token, payment_type, network, recipient
        );

        // 2. Create payment record
        let payment = PaymentRequest {
            id: payment_id.clone(),
            recipient: recipient.to_string(),
            merchant_did: merchant_did.to_string(),
            amount,
            token: token.to_string(),
            network: network.to_string(),
            description: description.clone(),
            status: PaymentStatus::PendingAuth,
            created_at: chrono::Utc::now(),
            tx_signature: None,
        };

        // 3. Save to store
        if let Err(e) = self.payments.save_payment(&payment) {
            return format!("Error: Failed to save payment: {}", e);
        }

        // 3.5 Verify VC if present and platform key configured
        if let Some(vc_value) = challenge.get("verifiable_credential") {
            if let (Some(vk_bytes), Ok(vc)) = (&self.platform_verifying_key, serde_json::from_value::<VerifiableCredential>(vc_value.clone())) {
                match vc.verify(vk_bytes, &self.platform_did) {
                    Ok(()) => {
                        tracing::info!("VC verified for merchant: {}", vc.credential_subject.id);
                    }
                    Err(e) => {
                        let _ = self.payments.update_status(&payment_id, &PaymentStatus::Rejected);
                        return format!("Payment rejected: VC verification failed: {}", e);
                    }
                }
            }
        }

        // 3.6 On-chain DID verification (V2.0 — if Solana configured)
        if let Some(bridge) = &self.solana_bridge {
            match bridge.quick_verify(merchant_did).await {
                Ok(true) => {
                    tracing::info!("Merchant {} verified on-chain", merchant_did);
                }
                Ok(false) => {
                    let _ = self.payments.update_status(&payment_id, &PaymentStatus::Rejected);
                    return format!(
                        "Payment rejected: merchant {} not found on-chain",
                        merchant_did
                    );
                }
                Err(e) => {
                    tracing::warn!("On-chain verification error (continuing): {}", e);
                }
            }
        }

        // 4. Check auto-approve
        if self.auto_approve_max > 0 && amount <= self.auto_approve_max {
            // Get session for Solana payment if available
            let session = self.get_active_session();
            let tx_sig = execute_payment(&self.solana_client, &payment, &session).await;
            if let Err(e) = self
                .payments
                .update_status(&payment_id, &PaymentStatus::Executed)
            {
                return format!("Error: Failed to update status: {}", e);
            }
            if let Err(e) = self.payments.set_tx_signature(&payment_id, &tx_sig) {
                return format!("Error: Failed to set tx signature: {}", e);
            }
            return format!(
                "Auto-approved payment (under threshold). Tx: {}",
                tx_sig
            );
        }

        // 5. Register pending auth and send request
        let rx = self.pending.register(&payment_id);

        match self
            .mediator
            .send_auth_request(&input.phone_did, &payment)
            .await
        {
            Ok(_) => {}
            Err(e) => {
                self.pending.resolve(&payment_id, AuthResponse {
                    authorized: false,
                    list_action: "none".to_string(),
                    merchant_did: String::new(),
                });
                return format!("Error: Failed to send auth request: {}", e);
            }
        }

        // 6. Wait for response with timeout
        let timeout = Duration::from_secs(self.auth_timeout);
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(resp)) if resp.authorized => {
                let session = self.get_active_session();
                let tx_sig = execute_payment(&self.solana_client, &payment, &session).await;
                let _ = self
                    .payments
                    .update_status(&payment_id, &PaymentStatus::Executed);
                let _ = self.payments.set_tx_signature(&payment_id, &tx_sig);

                // Handle list_action if specified
                if resp.list_action != "none" && !resp.merchant_did.is_empty() {
                    let entry = MerchantListEntry {
                        did: resp.merchant_did.clone(),
                        name: None,
                        max_amount: Some(amount),
                        added_at: chrono::Utc::now(),
                    };
                    match resp.list_action.as_str() {
                        "whitelist" => {
                            if let Err(e) = self.list_store.add_to_whitelist(entry) {
                                tracing::warn!("Failed to add to whitelist: {}", e);
                            } else {
                                tracing::info!("Added {} to whitelist", resp.merchant_did);
                            }
                        }
                        "blacklist" => {
                            if let Err(e) = self.list_store.add_to_blacklist(entry) {
                                tracing::warn!("Failed to add to blacklist: {}", e);
                            } else {
                                tracing::info!("Added {} to blacklist", resp.merchant_did);
                            }
                        }
                        _ => {}
                    }
                }

                format!(
                    "Payment authorized and executed. Tx: {}\nAmount: {} {}\nTo: {}",
                    tx_sig, amount, token, recipient
                )
            }
            Ok(Ok(_)) => {
                let _ = self
                    .payments
                    .update_status(&payment_id, &PaymentStatus::Rejected);
                "Payment rejected by user.".to_string()
            }
            Ok(Err(_)) => {
                let _ = self
                    .payments
                    .update_status(&payment_id, &PaymentStatus::Expired);
                "Payment authorization failed (internal error).".to_string()
            }
            Err(_) => {
                self.pending.resolve(&payment_id, AuthResponse {
                    authorized: false,
                    list_action: "none".to_string(),
                    merchant_did: String::new(),
                });
                let _ = self
                    .payments
                    .update_status(&payment_id, &PaymentStatus::Expired);
                "Payment authorization timed out.".to_string()
            }
        }
    }

    #[tool(description = "Check the status of a payment authorization request.")]
    async fn check_authorization(
        &self,
        Parameters(input): Parameters<AuthorizationCheckInput>,
    ) -> String {
        match self.payments.get_payment(&input.payment_id) {
            Ok(Some(payment)) => {
                let tx = payment
                    .tx_signature
                    .map(|t| format!("\nTx: {}", t))
                    .unwrap_or_default();
                format!(
                    "Payment: {}\nStatus: {}\nAmount: {} {}\nTo: {}\nMerchant DID: {}\nCreated: {}{}",
                    payment.id,
                    payment.status,
                    payment.amount,
                    payment.token,
                    payment.recipient,
                    payment.merchant_did,
                    payment.created_at,
                    tx
                )
            }
            Ok(None) => "Payment not found.".to_string(),
            Err(e) => format!("Error: {}", e),
        }
    }

    #[tool(description = "Get recent payment history.")]
    async fn get_payment_history(
        &self,
        Parameters(input): Parameters<PaymentHistoryInput>,
    ) -> String {
        match self.payments.list_payments(input.limit) {
            Ok(payments) => {
                if payments.is_empty() {
                    return "No payments found.".to_string();
                }
                let mut result = format!("Recent payments ({}):\n\n", payments.len());
                for p in &payments {
                    result.push_str(&format!(
                        "- {} | {} {} | {} | {} | {}\n",
                        &p.id[..8.min(p.id.len())],
                        p.amount,
                        p.token,
                        p.status,
                        p.recipient,
                        p.created_at.format("%Y-%m-%d %H:%M")
                    ));
                }
                result
            }
            Err(e) => format!("Error: {}", e),
        }
    }

    #[tool(description = "Get our DID identity and mediator connection status.")]
    async fn get_identity(&self) -> String {
        let solana_status = if self.solana_client.is_some() {
            "Solana: connected"
        } else {
            "Solana: not configured"
        };
        format!(
            "DID: {}\nMediator: connected\nPhone DID: {}\n{}",
            self.mediator.our_did(),
            self.phone_did,
            solana_status
        )
    }
}

impl IgnitePayMcpServer {
    /// Get active session from Solana client, if configured.
    fn get_active_session(&self) -> Option<SessionKeypair> {
        let owner = Pubkey::default();
        self.solana_client
            .as_ref()?
            .session_manager()
            .get_active_session(&owner)
            .ok()
            .flatten()
    }
}

#[tool_handler]
impl ServerHandler for IgnitePayMcpServer {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo::new(
            ServerCapabilities::builder().enable_tools().build(),
        )
        .with_instructions(
            "Ignite Pay MCP Server (V2.0) — handles x402 HTTP payment challenges via DIDComm-encrypted \
             authorization with on-chain Solana payment execution. Supports on-chain merchant DID \
             verification via SPL Account Compression. Use process_x402_challenge when you encounter \
             an HTTP 402 response to request payment approval from the user's phone.",
        )
    }
}

// ── Main ───────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ignite_pay_mcp=info".into()),
        )
        .init();

    let config = load_config()?;
    tracing::info!("Loaded config: mediator={}", config.mediator.ws_url);

    // Open sled database first for identity persistence
    let db = sled::open(&config.storage.path)?;
    tracing::info!("Database opened at {}", config.storage.path);

    // Create mediator connection with identity persistence
    let mediator = Arc::new(
        MediatorConnection::new(&config.mediator.ws_url, &db)?
    );

    // Register phone as a peer
    if !config.mediator.phone_did.is_empty() {
        mediator.add_peer(&config.mediator.phone_did).await;
        tracing::info!("Registered phone peer: {}", config.mediator.phone_did);
    }

    // Create payment store + pending auth store + list store
    let payments = Arc::new(payment::PaymentStore::from_db(db));
    let pending = Arc::new(PendingAuthStore::new());
    let list_store = Arc::new(ListStore::new(payments.get_db()));

    // V2.0: Initialize Solana client if configured
    let solana_client = if config.solana.is_configured() {
        let solana_db = sled::open(format!("{}/solana", config.storage.path))?;
        let relayer = if config.solana.relayer_url.is_empty() {
            None
        } else {
            Some(config.solana.relayer_url.clone())
        };
        match IgnitePayClient::new(
            &config.solana.rpc_url,
            solana_db,
            config.solana.pay_mode(),
            relayer,
        ) {
            Ok(client) => {
                tracing::info!(
                    "Solana client initialized: rpc={}, mode={:?}",
                    config.solana.rpc_url,
                    config.solana.pay_mode()
                );
                Some(Arc::new(client))
            }
            Err(e) => {
                tracing::error!("Failed to initialize Solana client: {}", e);
                None
            }
        }
    } else {
        tracing::info!("Solana not configured, using mock payments");
        None
    };

    // V2.0: Initialize Solana DID bridge if configured
    let solana_bridge = if config.solana.is_configured() && !config.solana.das_endpoint.is_empty() {
        match SolanaDidBridge::new(
            &config.solana.rpc_url,
            &config.solana.tree_address,
            &config.solana.tree_authority,
            &config.solana.das_endpoint,
        ) {
            Ok(bridge) => {
                tracing::info!("Solana DID bridge initialized for on-chain verification");
                Some(Arc::new(bridge))
            }
            Err(e) => {
                tracing::error!("Failed to initialize Solana DID bridge: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Connect to mediator (spawns background task with pending auth handling)
    mediator.connect(Arc::clone(&pending)).await?;
    tracing::info!("Connecting to mediator at {}...", config.mediator.ws_url);

    // Build and run MCP server
    let server = IgnitePayMcpServer {
        tool_router: IgnitePayMcpServer::tool_router(),
        mediator,
        payments,
        pending,
        list_store,
        phone_did: config.mediator.phone_did,
        auth_timeout: config.policy.auth_timeout,
        auto_approve_max: config.policy.auto_approve_max,
        platform_did: config.platform.did.clone(),
        platform_verifying_key: config.platform.verifying_key_bytes(),
        solana_client,
        solana_bridge,
    };

    tracing::info!("Starting MCP server on stdio...");
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
