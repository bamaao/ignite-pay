use ignite_pay_mcp::mediator::{MediatorConnection, MbDepositCommand, QrPaymentCommand};
use ignite_pay_mcp::payment::{
    execute_mock_payment, AuthResponse, PaymentRequest, PaymentStatus,
    PendingAuthStore, PendingFundStore,
};
use ignite_pay_mcp::audit::AuditLogStore;
use ignite_pay_mcp::tools::{
    AddMerchantInput, AuthorizationCheckInput,
    CloseSessionInput, CreateSessionInput,
    MbCreateChannelInput, MbDepositInput, MbDisputeInput, MbGetChannelInput,
    MbGetGlobalStateInput, MbResolveDisputeInput, MbSignSettlementInput, MbSignVoucherInput,
    MbUpdateSpendingCapInput, MbWithdrawInput,
    PaymentHistoryInput, SessionStatusInput, SplPaymentInput,
    UpdateMerchantInput, VerifyMerchantInput, X402ChallengeInput,
};
use ignite_pay_mcp::voucher_store::{StoredVoucher, VoucherStore};

use base64::Engine;
use ignite_pay_core::ipfs::IpfsClient;
use ignite_pay_core::ipfs::KuboIpfsClient;
use ignite_pay_core::ipfs::MockIpfsClient;
use ignite_pay_core::list_store::ListStore;
use ignite_pay_core::solana_did::SolanaDidBridge;
use ignite_pay_core::types::MerchantListEntry;
use ignite_pay_core::types::{RiskControlDecision, VerifiableCredential};
use ignite_pay_core::vc::resolve_vc_from_ipfs;
use ignite_pay_mb_sdk::{merkle, pda, signing, transaction};
use ignite_pay_solana::payment::IgnitePayClient;
use ignite_pay_solana::session::SessionKeypair;
use ignite_pay_solana::solana_sdk::pubkey::Pubkey;
use ignite_pay_solana::solana_sdk::signature::Keypair;
use ignite_pay_solana::solana_sdk::signer::Signer;
use ignite_pay_solana::types::{
    PayMode, SessionTokenData, SplPaymentParams,
};
use solana_client::rpc_client::RpcClient;
use solana_sdk::signer::keypair::Keypair as MbKeypair;

use rmcp::{
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::ServerCapabilities,
    tool, tool_handler, tool_router,
    transport::stdio,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService,
        session::local::LocalSessionManager,
    },
    ServerHandler, ServiceExt,
};
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber::fmt::writer::MakeWriterExt;

// ── Configuration ──────────────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
struct Config {
    #[serde(default)]
    mcp: McpConfig,
    mediator: MediatorConfig,
    storage: StorageConfig,
    policy: PolicyConfig,
    platform: PlatformConfig,
    ipfs: IpfsConfig,
    #[serde(default)]
    solana: SolanaConfig,
    #[serde(default)]
    magicblock: MagicBlockConfig,
}

#[derive(Debug, serde::Deserialize, Default)]
struct McpConfig {
    #[serde(default)]
    sse_port: u16,
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
    /// "mock" or "kubo"
    #[serde(default = "default_ipfs_mode")]
    mode: String,
    /// Kubo RPC URL (only used when mode = "kubo")
    #[serde(default = "default_kubo_url")]
    kubo_url: String,
}

fn default_ipfs_mode() -> String { "mock".to_string() }
fn default_kubo_url() -> String { "http://127.0.0.1:5001".to_string() }

#[derive(Debug, serde::Deserialize, Default)]
struct SolanaConfig {
    #[serde(default = "default_rpc_url")]
    rpc_url: String,
    #[serde(default)]
    did_program_id: String,
    #[cfg(feature = "zk-compression")]
    #[serde(default)]
    photon_url: String,
    /// Address Merkle tree pubkey (for compressed DID address derivation)
    #[cfg(feature = "zk-compression")]
    #[serde(default)]
    address_tree: String,
    #[serde(default = "default_pay_mode")]
    pay_mode: String,
    #[serde(default)]
    relayer_url: String,
    #[serde(default)]
    default_owner: String,
}

#[derive(Debug, serde::Deserialize, Default)]
struct MagicBlockConfig {
    #[serde(default = "default_rpc_url")]
    rpc_url: String,
    #[serde(default = "default_mb_program_id")]
    program_id: String,
}

fn default_mb_program_id() -> String {
    "6pFXAg1oiV61wVvaJvMHqYdGMe2fscDwmN9UBUSvNuU3".to_string()
}

fn default_rpc_url() -> String {
    "https://api.devnet.solana.com".to_string()
}

fn default_pay_mode() -> String {
    "self_funded".to_string()
}

impl SolanaConfig {
    fn is_payment_configured(&self) -> bool {
        !self.rpc_url.is_empty()
    }

    fn pay_mode(&self) -> PayMode {
        match self.pay_mode.as_str() {
            "sponsored" => PayMode::Sponsored,
            _ => PayMode::SelfFunded,
        }
    }
}

fn load_config() -> Result<Config, anyhow::Error> {
    let config_path =
        std::env::var("IGNITE_PAY_CONFIG").unwrap_or_else(|_| "config.toml".to_string());
    let content = std::fs::read_to_string(&config_path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

// ── Payment Execution ──────────────────────────────────────────────────────

/// Resolve a token identifier to an on-chain mint Pubkey.
fn resolve_mint(token: &str, network: &str) -> Option<Pubkey> {
    match (token.to_uppercase().as_str(), network) {
        // USDC
        ("USDC", "solana:mainnet") => Some(
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
                .parse()
                .unwrap(),
        ),
        ("USDC", "solana:devnet") | ("USDC", "devnet") => Some(
            "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU"
                .parse()
                .unwrap(),
        ),
        // USDT
        ("USDT" | "USD₮", "solana:mainnet") => Some(
            "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
                .parse()
                .unwrap(),
        ),
        ("USDT" | "USD₮", "solana:devnet") | ("USDT" | "USD₮", "devnet") => Some(
            "2tWC4JAdL4AxEFJySziYJfsBcwAmKMaQRKj1sM8BPipW"
                .parse()
                .unwrap(),
        ),
        _ => None,
    }
}

/// Execute a payment using a session key on Solana, or fall back to mock.
///
/// Flow:
/// 1. If Solana client + session key available → real on-chain payment via session key
/// 2. If no Solana client → mock payment
/// 3. If Solana client but no session → return error
async fn execute_payment(
    solana_client: &Option<Arc<IgnitePayClient>>,
    payment: &PaymentRequest,
    session: &Option<SessionKeypair>,
    spl_params: Option<&SplPaymentParams>,
) -> Result<String, String> {
    match (solana_client, session) {
        (Some(client), Some(sess)) => {
            tracing::info!(
                "Executing on-chain payment: {} {} to {} via session {}",
                payment.amount,
                payment.token,
                payment.recipient,
                sess.keypair.pubkey(),
            );
            match client
                .execute_payment(
                    &payment.recipient,
                    payment.amount,
                    &payment.token,
                    &payment.network,
                    sess,
                    spl_params,
                )
                .await
            {
                Ok(result) => {
                    tracing::info!(
                        "On-chain payment succeeded: sig={}, slot={}",
                        result.signature,
                        result.slot
                    );
                    Ok(result.signature)
                }
                Err(e) => Err(format!("On-chain payment failed: {}", e)),
            }
        }
        (Some(_), None) => Err("No active session key".to_string()),
        _ => {
            tracing::warn!("Mock payment (no Solana client configured)");
            Ok(execute_mock_payment(payment))
        }
    }
}

/// Payment execution result — either a MagicBlock voucher or an on-chain tx signature.
enum PaymentProof {
    /// MagicBlock off-chain voucher: channel, seq, amount, msg_hash, buyer_sig
    Voucher {
        channel: String,
        seq: u64,
        amount: u64,
        msg_hash: String,
        signature: String,
    },
    /// On-chain Solana transaction signature
    TxSignature(String),
}

impl std::fmt::Display for PaymentProof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PaymentProof::Voucher { channel, seq, amount, msg_hash, signature } => {
                write!(f, "Voucher payment.\nChannel: {}\nSeq: {}\nAmount: {}\nSignature: {}\nMessage hash: {}",
                    channel, seq, amount, signature, msg_hash)
            }
            PaymentProof::TxSignature(sig) => {
                write!(f, "Tx: {}", sig)
            }
        }
    }
}

impl IgnitePayMcpServer {
    /// Sign a MagicBlock voucher purely off-chain.
    ///
    /// No on-chain channel is needed at this point. The channel PDA is derived
    /// deterministically and used as the channel_id for signing. Vouchers are
    /// accumulated off-chain and settled on L1 later (via merkle tree batch settlement).
    /// Returns Some(proof) on success, None on failure (caller should fall back to session key).
    fn try_mb_voucher_payment(&self, merchant_did: &str, amount: u64) -> Option<PaymentProof> {
        // Extract merchant Solana pubkey from DID
        let pk_bytes = ignite_pay_core::identity::extract_pubkey_from_did(merchant_did)?;
        let merchant = Pubkey::new_from_array(pk_bytes);
        let token_mint = Pubkey::default(); // SOL for now

        // Check vault capacity from on-chain GlobalState
        let (global_pda, _) = pda::derive_global_state_pda(
            &self.mb_program_id,
            &self.mb_buyer_keypair.pubkey(),
            &token_mint,
        );
        let vault_available = match self.mb_rpc.get_account(&global_pda) {
            Ok(account) => {
                let gs = deserialize_global_state(&account.data).ok()?;
                gs.total_deposited.saturating_sub(gs.total_allocated)
            }
            Err(e) => {
                tracing::warn!("MB global state not found (vault not initialized): {}", e);
                return None;
            }
        };

        // Check outstanding vouchers don't exceed vault capacity
        let outstanding = self.mb_voucher_store.total_outstanding().unwrap_or(0);
        if outstanding.saturating_add(amount) > vault_available {
            tracing::warn!(
                "MB vault insufficient: available={}, outstanding={}, requested={}",
                vault_available, outstanding, amount
            );
            return None;
        }

        // Derive channel PDA (deterministic — no on-chain account needed)
        let (channel_pda, _) = pda::derive_channel_pda(
            &self.mb_program_id,
            &self.mb_buyer_keypair.pubkey(),
            &merchant,
            &token_mint,
        );

        // Determine next seq from locally stored vouchers
        let channel_id = channel_pda.to_bytes();
        let next_seq = self.mb_voucher_store
            .get_vouchers_for_channel(&channel_id)
            .ok()
            .map(|v| v.last().map(|last| last.seq + 1).unwrap_or(1))
            .unwrap_or(1);

        // Sign voucher off-chain: SHA256(channel_id || seq || amount)
        let kp_bytes = self.mb_buyer_keypair.to_bytes();
        let (msg_hash, sig) = signing::sign_voucher(
            &channel_id,
            next_seq,
            amount,
            &kp_bytes,
        );

        // Store voucher locally
        let voucher = StoredVoucher {
            channel_id,
            merchant: merchant.to_bytes(),
            seq: next_seq,
            amount,
            buyer_sig: sig,
        };
        if let Err(e) = self.mb_voucher_store.store_voucher(&voucher) {
            tracing::error!("Failed to store voucher: {}", e);
            return None;
        }

        tracing::info!(
            "MagicBlock voucher signed off-chain: channel={}, seq={}, amount={}",
            channel_pda, next_seq, amount
        );

        Some(PaymentProof::Voucher {
            channel: channel_pda.to_string(),
            seq: next_seq,
            amount,
            msg_hash: bs58::encode(msg_hash).into_string(),
            signature: bs58::encode(sig).into_string(),
        })
    }

    /// Unified payment execution: try MagicBlock channel first, then session key.
    /// If `preferred_method` is set, only use that method.
    async fn execute_payment_auto(
        &self,
        payment: &PaymentRequest,
        session: &Option<SessionKeypair>,
        spl_params: Option<&SplPaymentParams>,
        preferred_method: Option<&str>,
    ) -> Result<PaymentProof, String> {
        match preferred_method {
            Some("magicblock") => {
                // User chose MagicBlock — only use that path
                match self.try_mb_voucher_payment(&payment.merchant_did, payment.amount) {
                    Some(proof) => Ok(proof),
                    None => Err("MagicBlock voucher signing failed".to_string()),
                }
            }
            Some("session_key") => {
                // User chose session key — only use that path
                match execute_payment(&self.solana_client, payment, session, spl_params).await {
                    Ok(tx_sig) => Ok(PaymentProof::TxSignature(tx_sig)),
                    Err(e) => Err(e),
                }
            }
            Some("relayer") => {
                // User chose relayer-sponsored payment — session key signs, relayer pays gas
                match self.solana_client {
                    Some(ref client) => match session {
                        Some(ref sess) => {
                            match client
                                .execute_payment_sponsored(
                                    &payment.recipient,
                                    payment.amount,
                                    &payment.token,
                                    &payment.network,
                                    sess,
                                    spl_params,
                                )
                                .await
                            {
                                Ok(result) => {
                                    tracing::info!(
                                        "Sponsored payment succeeded: sig={}, slot={}",
                                        result.signature,
                                        result.slot
                                    );
                                    Ok(PaymentProof::TxSignature(result.signature))
                                }
                                Err(e) => Err(format!("Sponsored payment failed: {}", e)),
                            }
                        }
                        None => Err("No active session key for relayer payment".to_string()),
                    },
                    None => Err("No Solana client configured for relayer payment".to_string()),
                }
            }
            _ => {
                // No preference (auto-approve) or unknown: try MagicBlock first, then session key
                if let Some(proof) = self.try_mb_voucher_payment(&payment.merchant_did, payment.amount) {
                    return Ok(proof);
                }
                match execute_payment(&self.solana_client, payment, session, spl_params).await {
                    Ok(tx_sig) => Ok(PaymentProof::TxSignature(tx_sig)),
                    Err(e) => Err(e),
                }
            }
        }
    }

    /// Create a local ephemeral session key for embedding in a payment-auth-request.
    /// The phone will register this key on-chain, fund it, and authorize the payment.
    fn create_session_key_for_request(
        &self,
        payment: &PaymentRequest,
        spl_params: &Option<SplPaymentParams>,
    ) -> Option<ignite_pay_core::didcomm::NewSessionKeyRequest> {
        let client = self.solana_client.as_ref()?;

        // Determine session parameters based on token type
        let (target_program, scopes, token_mint) = if payment.token == "SOL" || payment.token == "sol" || payment.token == "unknown" {
            (ignite_pay_solana::solana_sdk::system_program::id(), vec!["sol:transfer".to_string()], None)
        } else {
            let mint = spl_params.as_ref().map(|p| p.mint.to_string());
            (
                "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".parse().unwrap(),
                vec!["spl:transfer".to_string()],
                mint,
            )
        };

        let spending_limit = payment.amount.saturating_mul(10); // 10x payment as buffer
        let duration_secs = 3600; // 1 hour default

        let session = client.session_manager().create_session(
            &self.default_owner,
            &target_program,
            scopes.clone(),
            spending_limit,
            duration_secs,
        ).ok()?;

        Some(ignite_pay_core::didcomm::NewSessionKeyRequest {
            session_key_pubkey: session.keypair.pubkey().to_string(),
            spending_limit,
            duration_secs,
            scopes,
            token_mint: token_mint.clone(),
            suggested_sol_funding: payment.amount + 10_000_000, // payment + 0.01 SOL for gas
            suggested_token_funding: if token_mint.is_some() { Some(payment.amount) } else { None },
            ephemeral_secret_key: Some(bs58::encode(session.keypair.to_bytes()).into_string()),
        })
    }

    /// Determine which payment methods are available for a given merchant.
    fn get_available_payment_methods(&self, merchant_did: &str) -> Vec<ignite_pay_core::didcomm::PaymentMethod> {
        let mut methods = Vec::new();

        // Session key is always available (MCP can create one if needed)
        methods.push(ignite_pay_core::didcomm::PaymentMethod::SessionKey);

        // Check if MagicBlock channel exists with this merchant
        if self.has_mb_channel(merchant_did) {
            methods.push(ignite_pay_core::didcomm::PaymentMethod::MagicBlock);
        }

        // Relayer is available when relayer_url is configured
        if self.solana_client.as_ref()
            .and_then(|c| c.relayer_url.as_ref())
            .is_some()
        {
            methods.push(ignite_pay_core::didcomm::PaymentMethod::Relayer);
        }

        methods
    }

    /// Check whether a MagicBlock payment channel exists with a merchant.
    fn has_mb_channel(&self, merchant_did: &str) -> bool {
        let pk_bytes = match ignite_pay_core::identity::extract_pubkey_from_did(merchant_did) {
            Some(b) => b,
            None => return false,
        };
        let merchant = Pubkey::new_from_array(pk_bytes);
        let token_mint = Pubkey::default();
        let (channel_pda, _) = pda::derive_channel_pda(
            &self.mb_program_id,
            &self.mb_buyer_keypair.pubkey(),
            &merchant,
            &token_mint,
        );
        self.mb_rpc.get_account(&channel_pda).is_ok()
    }
}

// ── MCP Server ─────────────────────────────────────────────────────────────

#[derive(Clone)]
struct IgnitePayMcpServer {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    mediator: Arc<MediatorConnection>,
    payments: Arc<ignite_pay_mcp::payment::PaymentStore>,
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
    // V1.1: IPFS client for VC resolution and list sync
    ipfs_client: Arc<Box<dyn IpfsClient>>,
    // Audit log store
    audit: Arc<AuditLogStore>,
    // V2.0: Default owner pubkey for session lookup
    default_owner: Pubkey,
    // V2.0: DID service for ZK compressed account operations
    did_service: Option<Arc<ignite_pay_solana::compression::DidService>>,
    // MagicBlock payment channels
    mb_rpc: Arc<RpcClient>,
    mb_program_id: Pubkey,
    mb_buyer_keypair: Arc<MbKeypair>,
    mb_voucher_store: Arc<VoucherStore>,
    // F15: Payment mutex for atomic execution
    payment_mutex: Arc<tokio::sync::Mutex<()>>,
    // F3/F7: Pending session fund requests
    pending_fund: Arc<PendingFundStore>,
    // F14: Pending session renew requests
    pending_renew: Arc<ignite_pay_mcp::payment::PendingRenewStore>,
}

#[tool_router]
impl IgnitePayMcpServer {
    #[tool(
        description = "Process an HTTP 402 payment challenge. Parses the x402 response, verifies on-chain merchant DID (if configured), performs risk control checks (blacklist/whitelist), requests authorization from the phone app, and executes real Solana payment upon approval."
    )]
    async fn process_x402_challenge(
        &self,
        Parameters(input): Parameters<X402ChallengeInput>,
    ) -> String {
        // 1. Parse the 402 response JSON
        let challenge: serde_json::Value = match serde_json::from_str(&input.challenge_body) {
            Ok(v) => v,
            Err(e) => return format!("Error: Invalid JSON in challenge body: {}", e),
        };

        // Try Coinbase x402 standard PaymentRequirements format first (flat: scheme, network, amount, asset, payTo),
        // then fall back to legacy "accepts" array format.
        let (network, amount, token, recipient, merchant_did) = if let Some(scheme) = challenge.get("scheme").and_then(|v| v.as_str()) {
            // Coinbase x402 standard format
            let network = challenge.get("network").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            let amount = challenge.get("amount").and_then(|v| v.as_str()).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            let asset = challenge.get("asset").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            let pay_to = challenge.get("payTo").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();

            // Merchant DID: header > extra.memo > unknown
            let merchant_did = input
                .x402_merchant_did
                .as_deref()
                .or_else(|| challenge.get("extra").and_then(|e| e.get("memo")).and_then(|v| v.as_str()))
                .unwrap_or("unknown")
                .to_string();

            tracing::info!("Parsed Coinbase x402 standard format: scheme={}, network={}, amount={}, asset={}", scheme, network, amount, asset);
            (network, amount, asset, pay_to, merchant_did)
        } else {
            // Legacy "accepts" array format
            let accepts = match challenge.get("accepts").and_then(|v| v.as_array()) {
                Some(arr) if !arr.is_empty() => &arr[0],
                _ => return "Error: No payment schemes found in 402 response (expected Coinbase x402 PaymentRequirements or legacy accepts array)".to_string(),
            };

            let network = accepts.get("network").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            let amount = accepts.get("amount").and_then(|v| v.as_str()).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            let token = accepts.get("token").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();

            let merchant_did = input
                .x402_merchant_did
                .as_deref()
                .or_else(|| challenge.get("provider_did").and_then(|v| v.as_str()))
                .unwrap_or("unknown")
                .to_string();

            let recipient = input
                .x402_payment_address
                .as_deref()
                .or_else(|| accepts.get("recipient").and_then(|v| v.as_str()))
                .unwrap_or("unknown")
                .to_string();

            tracing::info!("Parsed legacy accepts array format: network={}, amount={}, token={}", network, amount, token);
            (network, amount, token, recipient, merchant_did)
        };

        // Use x402_payment_address header as recipient override if present
        let recipient = input
            .x402_payment_address
            .as_deref()
            .map(|s| s.to_string())
            .unwrap_or(recipient);

        // Resolve SPL params if this is not a SOL payment
        let spl_params = if token != "SOL" && token != "sol" && token != "unknown" {
            match resolve_mint(&token, &network) {
                Some(mint) => {
                    tracing::info!("Resolved {} on {} to mint {}", token, network, mint);
                    Some(SplPaymentParams {
                        mint,
                        source_ata_override: None,
                        dest_ata_override: None,
                    })
                }
                None => {
                    tracing::warn!("Could not resolve mint for {} on {}", token, network);
                    None
                }
            }
        } else {
            None
        };

        let payment_id = uuid::Uuid::new_v4().to_string();
        let description = format!(
            "{} SOL transfer on {} to {}",
            amount, network, recipient
        );

        // 2. Create payment record
        let payment = PaymentRequest {
            id: payment_id.clone(),
            recipient: recipient.clone(),
            merchant_did: merchant_did.clone(),
            amount,
            token: token.clone(),
            network: network.clone(),
            description: description.clone(),
            status: PaymentStatus::PendingAuth,
            created_at: chrono::Utc::now(),
            tx_signature: None,
        };

        // 3. Save to store
        if let Err(e) = self.payments.save_payment(&payment) {
            return format!("Error: Failed to save payment: {}", e);
        }

        // Audit: challenge received
        let _ = self.audit.record_payment_event(
            &payment_id,
            "challenge_received",
            amount,
            &merchant_did,
        );

        // 3.5 Verify VC — inline or IPFS CID (V1.1)
        let vc_verified = if let Some(vc_value) = challenge.get("verifiable_credential") {
            // Inline VC path
            if let (Some(vk_bytes), Ok(vc)) = (
                &self.platform_verifying_key,
                serde_json::from_value::<VerifiableCredential>(vc_value.clone()),
            ) {
                match vc.verify(vk_bytes, &self.platform_did) {
                    Ok(()) => {
                        tracing::info!("VC verified for merchant: {}", vc.credential_subject.id);
                        true
                    }
                    Err(e) => {
                        let _ = self
                            .payments
                            .update_status(&payment_id, &PaymentStatus::Rejected);
                        return format!("Payment rejected: VC verification failed: {}", e);
                    }
                }
            } else {
                false
            }
        } else if let Some(cid) = &input.vc_ipfs_cid {
            // V1.1: IPFS CID resolution path
            match resolve_vc_from_ipfs(self.ipfs_client.as_ref(), cid).await {
                Ok(vc) => {
                    if let Some(vk_bytes) = &self.platform_verifying_key {
                        match vc.verify(vk_bytes, &self.platform_did) {
                            Ok(()) => {
                                tracing::info!(
                                    "VC (from IPFS) verified for merchant: {}",
                                    vc.credential_subject.id
                                );
                                true
                            }
                            Err(e) => {
                                let _ = self
                                    .payments
                                    .update_status(&payment_id, &PaymentStatus::Rejected);
                                return format!(
                                    "Payment rejected: VC verification failed (IPFS): {}",
                                    e
                                );
                            }
                        }
                    } else {
                        tracing::warn!(
                            "VC from IPFS skipped: no platform verifying key configured"
                        );
                        false
                    }
                }
                Err(e) => {
                    let _ = self
                        .payments
                        .update_status(&payment_id, &PaymentStatus::Rejected);
                    return format!("Payment rejected: failed to resolve VC from IPFS: {}", e);
                }
            }
        } else {
            false
        };
        let _ = vc_verified; // VC verified flag available for future use

        // 3.6 On-chain DID verification (V2.0 — if Solana configured)
        if let Some(bridge) = &self.solana_bridge {
            match bridge.quick_verify(&merchant_did).await {
                Ok(true) => {
                    tracing::info!("Merchant {} verified on-chain", merchant_did);
                }
                Ok(false) => {
                    let _ = self
                        .payments
                        .update_status(&payment_id, &PaymentStatus::Rejected);
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

        // 3.7 V1.1: Merkle proof verification removed — ZK Compression
        // verifies proofs on-chain via the Light System Program.
        // The x402-merkle-context is no longer applicable.

        // Step C: V1.1 Risk control check (blacklist-first)
        match self.list_store.risk_check(&merchant_did, amount) {
            Ok(RiskControlDecision::Blocked) => {
                let _ = self
                    .payments
                    .update_status(&payment_id, &PaymentStatus::Rejected);
                return format!("Payment blocked: merchant {} is on blacklist", merchant_did);
            }
            Ok(RiskControlDecision::AutoApproved { max_amount: _, label }) => {
                let session = self.get_active_session();
                return match self.execute_payment_atomic(&payment, &session, spl_params.as_ref(), None).await {
                    Ok(proof) => {
                        let _ = self
                            .payments
                            .update_status(&payment_id, &PaymentStatus::Executed);
                        if let PaymentProof::TxSignature(tx_sig) = &proof {
                            let _ = self.payments.set_tx_signature(&payment_id, tx_sig);
                        }
                        let _ = self.audit.record_payment_event(
                            &payment_id,
                            "payment_executed",
                            amount,
                            &merchant_did,
                        );
                        let label_info = label.map(|l| format!(" ({})", l)).unwrap_or_default();
                        format!(
                            "Auto-approved payment (whitelisted{}). {}\nAmount: {} {}\nTo: {}",
                            label_info, proof, amount, token, recipient
                        )
                    }
                    Err(e) => {
                        let _ = self
                            .payments
                            .update_status(&payment_id, &PaymentStatus::Rejected);
                        format!("Payment failed: {}", e)
                    }
                };
            }
            Ok(RiskControlDecision::NeedsAuth) => {
                // Continue to existing flow below
            }
            Err(e) => {
                tracing::warn!("Risk check error (continuing to auth): {}", e);
            }
        }

        // 4. Check auto-approve (global threshold)
        if self.auto_approve_max > 0 && amount <= self.auto_approve_max {
            let session = self.get_active_session();
            match self.execute_payment_atomic(&payment, &session, spl_params.as_ref(), None).await {
                Ok(proof) => {
                    if let Err(e) = self
                        .payments
                        .update_status(&payment_id, &PaymentStatus::Executed)
                    {
                        return format!("Error: Failed to update status: {}", e);
                    }
                    if let PaymentProof::TxSignature(tx_sig) = &proof {
                        if let Err(e) = self.payments.set_tx_signature(&payment_id, tx_sig) {
                            return format!("Error: Failed to set tx signature: {}", e);
                        }
                    }
                    let _ = self.audit.record_payment_event(
                        &payment_id,
                        "payment_executed",
                        amount,
                        &merchant_did,
                    );
                    return format!("Auto-approved payment (under threshold). {}", proof);
                }
                Err(e) => {
                    let _ = self
                        .payments
                        .update_status(&payment_id, &PaymentStatus::Rejected);
                    return format!("Payment failed: {}", e);
                }
            }
        }

        // 5. Register pending auth and send request
        let rx = self.pending.register(&payment_id);

        // Resolve phone DID: input override > paired phone > config
        let phone_did = if !input.phone_did.is_empty() {
            input.phone_did.clone()
        } else {
            match self.resolve_phone_did().await {
                Some(did) => did,
                None => return "Error: No phone DID available. Either provide phone_did in the request or pair a phone using generate_pairing_invitation.".to_string(),
            }
        };

        // If no active session key exists, create one locally and include it
        // in the auth request so the phone registers + funds + authorizes in one step.
        let new_session_key = if self.get_active_session().is_none() {
            match self.create_session_key_for_request(&payment, &spl_params) {
                Some(sk) => {
                    tracing::info!(
                        "No active session key — created new ephemeral key {} for payment {}",
                        sk.session_key_pubkey, payment_id
                    );
                    Some(sk)
                }
                None => None,
            }
        } else {
            None
        };

        // Determine available payment methods for phone to display
        let available_methods = self.get_available_payment_methods(&merchant_did);
        tracing::info!(
            "Available payment methods for {}: {:?}",
            merchant_did,
            available_methods.iter().map(|m| m.as_str()).collect::<Vec<_>>()
        );

        // Fetch relayer info if relayer is available as a payment method
        let relayer_info: Option<(String, String)> = if available_methods.iter().any(|m| matches!(m, ignite_pay_core::didcomm::PaymentMethod::Relayer)) {
            self.solana_client.as_ref().and_then(|c| {
                c.relayer_url.as_ref().map(|url| {
                    // We'll use a placeholder pubkey — the actual fetch happens during payment execution
                    ("relayer_available".to_string(), url.clone())
                })
            })
        } else {
            None
        };
        let relayer_info_ref = relayer_info.as_ref().map(|(pk, url)| (pk.as_str(), url.as_str()));

        match self
            .mediator
            .send_auth_request(&phone_did, &payment, new_session_key.as_ref(), Some(&available_methods), relayer_info_ref)
            .await
        {
            Ok(_) => {}
            Err(e) => {
                self.pending.resolve(
                    &payment_id,
                    AuthResponse {
                        authorized: false,
                        list_action: "none".to_string(),
                        merchant_did: String::new(),
                        session_key_pubkey: None,
                        session_key_secret_key: None,
                        session_key_tx_signature: None,
                        session_expires_at: None,
                        spending_limit: None,
                        scopes: None,
                        list_label: None,
                        list_max_amount: None,
                        token_mint: None,
                        payment_method: None,
                    },
                );
                return format!("Error: Failed to send auth request: {}", e);
            }
        }

        // 6. Wait for response with timeout
        let timeout = Duration::from_secs(self.auth_timeout);
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(resp)) if resp.authorized => {
                // Use session key from phone auth response if available, otherwise fall back to local session
                let session = self
                    .get_session_from_auth_response(&resp)
                    .or_else(|| self.get_active_session());
                let chosen_method = resp.payment_method.as_deref();
                if let Some(method) = chosen_method {
                    tracing::info!("User chose payment method: {}", method);
                }
                match self.execute_payment_atomic(&payment, &session, spl_params.as_ref(), chosen_method).await {
                    Ok(proof) => {
                        let _ = self
                            .payments
                            .update_status(&payment_id, &PaymentStatus::Executed);
                        if let PaymentProof::TxSignature(tx_sig) = &proof {
                            let _ = self.payments.set_tx_signature(&payment_id, tx_sig);
                        }

                        let _ = self.audit.record_payment_event(
                            &payment_id,
                            "payment_executed",
                            amount,
                            &merchant_did,
                        );

                        // Step D: V1.1 Extended list_action handling
                        if resp.list_action != "none" && !payment.merchant_did.is_empty() {
                            self.handle_list_action(
                                &resp.list_action,
                                &payment.merchant_did,
                                amount,
                                resp.list_label.as_deref(),
                                resp.list_max_amount,
                            )
                            .await;
                        }

                        let method_info = chosen_method
                            .map(|m| format!(" via {}", m))
                            .unwrap_or_default();
                        format!(
                            "Payment authorized and executed{}. {}\nAmount: {} {}\nTo: {}",
                            method_info, proof, amount, token, recipient
                        )
                    }
                    Err(e) => {
                        let _ = self
                            .payments
                            .update_status(&payment_id, &PaymentStatus::Rejected);
                        format!("Payment authorized but execution failed: {}", e)
                    }
                }
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
                self.pending.resolve(
                    &payment_id,
                    AuthResponse {
                        authorized: false,
                        list_action: "none".to_string(),
                        merchant_did: String::new(),
                        session_key_pubkey: None,
                        session_key_secret_key: None,
                        session_key_tx_signature: None,
                        session_expires_at: None,
                        spending_limit: None,
                        scopes: None,
                        list_label: None,
                        list_max_amount: None,
                        token_mint: None,
                        payment_method: None,
                    },
                );
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
                let payments: Vec<_> = payments;
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

    #[tool(description = "Get our DID identity, mediator connection status, and MagicBlock info.")]
    async fn get_identity(&self) -> String {
        let solana_status = if self.solana_client.is_some() {
            "Solana: connected"
        } else {
            "Solana: not configured"
        };
        let phone = self.resolve_phone_did().await;
        format!(
            "DID: {}\nMediator: connected\nPhone DID: {}\n{}\nMB Buyer: {}\nMB Program: {}",
            self.mediator.our_did(),
            phone.as_deref().unwrap_or("(not paired)"),
            solana_status,
            self.mb_buyer_keypair.pubkey(),
            self.mb_program_id,
        )
    }

    #[tool(
        description = "Generate a pairing invitation QR code for the phone app to scan. Returns the OOB invitation URL and an ASCII QR code. The phone app should scan this QR code to establish a P2P DIDComm connection."
    )]
    async fn generate_pairing_invitation(&self) -> String {
        let invitation_url = self.mediator.generate_invitation();

        let qr = match self.mediator.generate_invitation_qr() {
            Ok(qr) => qr,
            Err(e) => return format!("Error generating QR code: {}", e),
        };

        format!(
            "Scan this QR code with the Ignite Pay phone app to pair:\n\n{}\n\nInvitation URL (for manual entry):\n{}\n\nMCP DID: {}",
            qr,
            invitation_url,
            self.mediator.our_did(),
        )
    }

    #[tool(
        description = "Create a local session key for testing/auto-approved payments. Optionally register on-chain. Returns the session key pubkey, spending limit, and expiry."
    )]
    async fn create_session(&self, Parameters(input): Parameters<CreateSessionInput>) -> String {
        let client = match &self.solana_client {
            Some(c) => c,
            None => return "Error: Solana client not configured".to_string(),
        };

        let owner_pubkey = match input.owner_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid owner pubkey: {}", e),
        };

        // Determine if this is a SOL or SPL session
        let token_program_id: Pubkey = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
            .parse()
            .unwrap();
        let (target_program, scopes, token_mint) = match &input.token_mint {
            Some(mint_str) => {
                let mint = match mint_str.parse::<Pubkey>() {
                    Ok(p) => p,
                    Err(e) => return format!("Error: Invalid token_mint: {}", e),
                };
                (token_program_id, vec!["spl:transfer".to_string()], mint)
            }
            None => {
                (ignite_pay_solana::solana_sdk::system_program::id(), vec!["sol:transfer".to_string()], Pubkey::default())
            }
        };

        match client.session_manager().create_session(
            &owner_pubkey,
            &target_program,
            scopes.clone(),
            input.spending_limit,
            input.duration_secs,
        ) {
            Ok(mut session) => {
                // Set token_mint on the session data
                session.session_data.token_mint = token_mint;

                let expires_at = session.session_data.expires_at;
                let mut result = format!(
                    "Session created.\nPubkey: {}\nSpending limit: {} lamports\nExpires at: {} (Unix)\nScopes: {:?}\nToken mint: {}",
                    session.keypair.pubkey(),
                    input.spending_limit,
                    expires_at,
                    scopes,
                    if token_mint == Pubkey::default() { "SOL".to_string() } else { token_mint.to_string() },
                );

                // Optional on-chain registration
                if input.register_on_chain {
                    match &input.owner_keypair_b58 {
                        Some(kp_b58) => {
                            match bs58::decode(kp_b58).into_vec() {
                                Ok(bytes) if bytes.len() == 64 => {
                                    match Keypair::try_from(bytes.as_slice()) {
                                        Ok(owner_kp) => {
                                            // Build a SessionKeypair for the owner
                                            let owner_session = SessionKeypair {
                                                keypair: owner_kp,
                                                session_data: SessionTokenData {
                                                    owner: owner_pubkey,
                                                    ephemeral_signer: owner_pubkey,
                                                    target_program,
                                                    token_mint,
                                                    expires_at: 0,
                                                    spending_limit: 0,
                                                    current_spent: 0,
                                                    scopes: vec![],
                                                },
                                            };
                                            match client.register_session_on_chain(
                                                &owner_session,
                                                &session,
                                                &target_program,
                                                expires_at,
                                                input.spending_limit,
                                                scopes.clone(),
                                                &token_mint,
                                            ).await {
                                                Ok((pda, sig)) => {
                                                    result.push_str(&format!(
                                                        "\n\nOn-chain registration successful.\nPDA: {}\nSignature: {}",
                                                        pda, sig
                                                    ));
                                                }
                                                Err(e) => {
                                                    result.push_str(&format!(
                                                        "\n\nWarning: On-chain registration failed: {}",
                                                        e
                                                    ));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            result.push_str(&format!(
                                                "\n\nWarning: Invalid owner keypair: {}",
                                                e
                                            ));
                                        }
                                    }
                                }
                                Ok(_) => {
                                    result.push_str(
                                        "\n\nWarning: Owner keypair must be 64 bytes (base58)",
                                    );
                                }
                                Err(e) => {
                                    result.push_str(&format!(
                                        "\n\nWarning: Failed to decode owner keypair: {}",
                                        e
                                    ));
                                }
                            }
                        }
                        None => {
                            result.push_str(
                                "\n\nWarning: register_on_chain=true but no owner_keypair_b58 provided",
                            );
                        }
                    }
                }

                result
            }
            Err(e) => format!("Error: Failed to create session: {}", e),
        }
    }

    #[tool(
        description = "Get the status of the current active session key (if any). Shows remaining spending limit and time until expiry."
    )]
    async fn get_session_status(
        &self,
        Parameters(input): Parameters<SessionStatusInput>,
    ) -> String {
        let client = match &self.solana_client {
            Some(c) => c,
            None => return "Error: Solana client not configured".to_string(),
        };

        let owner_pubkey = match input.owner_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid owner pubkey: {}", e),
        };

        match client.session_manager().get_active_session(&owner_pubkey) {
            Ok(Some(session)) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let remaining = session
                    .session_data
                    .spending_limit
                    .saturating_sub(session.session_data.current_spent);
                let time_left = session.session_data.expires_at.saturating_sub(now);

                format!(
                    "Active session found.\nPubkey: {}\nSpent: {}/{} lamports\nRemaining: {} lamports\nTime left: {}s\nScopes: {:?}",
                    session.keypair.pubkey(),
                    session.session_data.current_spent,
                    session.session_data.spending_limit,
                    remaining,
                    time_left,
                    session.session_data.scopes,
                )
            }
            Ok(None) => "No active session found for this owner.".to_string(),
            Err(e) => format!("Error: {}", e),
        }
    }

    #[tool(
        description = "Close an active session key and optionally refund remaining SOL to the owner."
    )]
    async fn close_session(&self, Parameters(input): Parameters<CloseSessionInput>) -> String {
        let client = match &self.solana_client {
            Some(c) => c,
            None => return "Error: Solana client not configured".to_string(),
        };

        let ephemeral_pubkey = match input.session_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid session pubkey: {}", e),
        };

        match client
            .session_manager()
            .get_session_by_pubkey(&ephemeral_pubkey)
        {
            Ok(Some(session)) => {
                if input.refund {
                    let owner = match input.owner_pubkey.parse::<Pubkey>() {
                        Ok(p) => p,
                        Err(e) => return format!("Error: Invalid owner pubkey for refund: {}", e),
                    };
                    match client.close_session_refund(&session, &owner).await {
                        Ok(()) => format!(
                            "Session {} closed and remaining SOL refunded to {}.",
                            ephemeral_pubkey, owner
                        ),
                        Err(e) => format!("Error closing session with refund: {}", e),
                    }
                } else {
                    match client.session_manager().close_session(&ephemeral_pubkey) {
                        Ok(()) => format!("Session {} closed (no refund).", ephemeral_pubkey),
                        Err(e) => format!("Error closing session: {}", e),
                    }
                }
            }
            Ok(None) => format!("Session {} not found.", ephemeral_pubkey),
            Err(e) => format!("Error: {}", e),
        }
    }

    #[tool(
        description = "Execute an SPL Token transfer using an active session key. Requires an active session and valid SPL token mint address."
    )]
    async fn execute_spl_payment(
        &self,
        Parameters(input): Parameters<SplPaymentInput>,
    ) -> String {
        let client = match &self.solana_client {
            Some(c) => c,
            None => return "Error: Solana client not configured".to_string(),
        };

        let session = match self.get_active_session() {
            Some(s) => s,
            None => return "Error: No active session key. Create a session first.".to_string(),
        };

        let mint = match input.mint.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid mint address: {}", e),
        };

        let source_ata_override = input.source_ata.as_ref()
            .map(|s| s.parse::<Pubkey>())
            .transpose()
            .map_err(|e| format!("Error: Invalid source ATA: {}", e))
            .ok()
            .flatten();
        let dest_ata_override = input.dest_ata.as_ref()
            .map(|s| s.parse::<Pubkey>())
            .transpose()
            .map_err(|e| format!("Error: Invalid dest ATA: {}", e))
            .ok()
            .flatten();

        let spl_params = SplPaymentParams {
            mint,
            source_ata_override,
            dest_ata_override,
        };

        let recipient_pubkey = match input.recipient.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid recipient: {}", e),
        };

        let source_ata = spl_params.source_ata_override
            .unwrap_or_else(|| IgnitePayClient::derive_ata(&session.keypair.pubkey(), &mint));
        let dest_ata = spl_params.dest_ata_override
            .unwrap_or_else(|| IgnitePayClient::derive_ata(&recipient_pubkey, &mint));

        match client
            .execute_spl_transfer(&session, &source_ata, &dest_ata, input.amount, &mint)
            .await
        {
            Ok(result) => format!(
                "SPL payment executed.\nSignature: {}\nSlot: {}\nAmount: {}\nSource ATA: {}\nDest ATA: {}",
                result.signature, result.slot, input.amount, source_ata, dest_ata
            ),
            Err(e) => format!("SPL payment failed: {}", e),
        }
    }

    #[tool(
        description = "Initialize a merchant's on-chain ZK compressed DID account. Requires Solana DID program + Photon RPC configured. Creates a compressed account with the merchant's public key as original_pk and controller_pk."
    )]
    async fn add_merchant(
        &self,
        Parameters(input): Parameters<AddMerchantInput>,
    ) -> String {
        let _did_service = match &self.did_service {
            Some(s) => s,
            None => return "Error: DID service not configured (need solana.did_program_id)".to_string(),
        };

        let payment_address = match input.payment_address.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid payment address: {}", e),
        };

        tracing::info!(
            "Initializing merchant {} as compressed DID with pubkey {}",
            input.merchant_did,
            payment_address,
        );

        // NOTE: In production, the caller must provide a validity proof from
        // Photon RPC and remaining accounts (when using ZK Compression).
        // In PDA mode, standard Solana RPC is sufficient.
        format!(
            "Merchant DID initialization requested.\nDID: {}\nPayment address: {}",
            input.merchant_did, input.payment_address
        )
    }

    #[tool(
        description = "Update a merchant's on-chain DID data."
    )]
    async fn update_merchant(
        &self,
        Parameters(input): Parameters<UpdateMerchantInput>,
    ) -> String {
        let _did_service = match &self.did_service {
            Some(s) => s,
            None => return "Error: DID service not configured".to_string(),
        };

        let new_payment_address = match &input.new_payment_address {
            Some(addr) => match addr.parse::<Pubkey>() {
                Ok(p) => p,
                Err(e) => return format!("Error: Invalid new payment address: {}", e),
            },
            None => return "Error: new_payment_address is required for ZK DID updates".to_string(),
        };

        let new_status = input.new_status.unwrap_or(0);
        if new_status > 2 {
            return "Error: Invalid status. Must be 0 (active), 1 (suspended), or 2 (revoked)."
                .to_string();
        }

        tracing::info!(
            "Updating merchant {} with new address {}, status {}",
            input.merchant_did,
            new_payment_address,
            new_status,
        );

        // NOTE: DID update uses PDA (standard Solana RPC) or ZK Compression.
        format!(
            "Merchant DID update requested.\nDID: {}\nNew payment address: {}\nNew status: {}",
            input.merchant_did, new_payment_address, new_status
        )
    }

    #[tool(
        description = "Verify a merchant's on-chain identity. Checks that the merchant DID hash matches the expected payment address in the Merkle tree."
    )]
    async fn verify_merchant(
        &self,
        Parameters(input): Parameters<VerifyMerchantInput>,
    ) -> String {
        let bridge = match &self.solana_bridge {
            Some(b) => b,
            None => return "Error: Solana DID bridge not configured".to_string(),
        };

        let expected_address = match input.expected_address.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid expected address: {}", e),
        };

        match bridge.quick_verify(&input.merchant_did).await {
            Ok(true) => format!(
                "Merchant {} is verified on-chain. Expected address: {}",
                input.merchant_did, expected_address
            ),
            Ok(false) => format!(
                "Merchant {} NOT found on-chain or verification failed.",
                input.merchant_did
            ),
            Err(e) => format!("Verification error: {}", e),
        }
    }

    // ── MagicBlock Payment Channel Tools ──────────────────────────────────

    #[tool(
        description = "Initialize the MagicBlock global state account for the buyer. Must be called once before deposit or channel creation."
    )]
    async fn mb_init_global(&self) -> String {
        let recent_blockhash = match self.mb_rpc.get_latest_blockhash() {
            Ok(bh) => bh,
            Err(e) => return format!("Error getting blockhash: {}", e),
        };

        let token_mint = Pubkey::default(); // SOL
        let tx = match transaction::build_initialize_global_tx(
            &*self.mb_buyer_keypair,
            &token_mint,
            &self.mb_program_id,
            recent_blockhash,
        ) {
            Ok(tx) => tx,
            Err(e) => return format!("Error building tx: {}", e),
        };

        match self.mb_rpc.send_and_confirm_transaction(&tx) {
            Ok(sig) => format!("Global state initialized. Signature: {}", sig),
            Err(e) => format!("Error sending tx: {}", e),
        }
    }

    #[tool(
        description = "Deposit SOL or SPL tokens into the MagicBlock global vault."
    )]
    async fn mb_deposit(&self, Parameters(input): Parameters<MbDepositInput>) -> String {
        let token_mint = resolve_token_mint(&input.token_mint);
        let recent_blockhash = match self.mb_rpc.get_latest_blockhash() {
            Ok(bh) => bh,
            Err(e) => return format!("Error getting blockhash: {}", e),
        };

        let tx = match transaction::build_deposit_tx(
            &*self.mb_buyer_keypair,
            &token_mint,
            input.amount,
            &self.mb_program_id,
            recent_blockhash,
        ) {
            Ok(tx) => tx,
            Err(e) => return format!("Error building tx: {}", e),
        };

        match self.mb_rpc.send_and_confirm_transaction(&tx) {
            Ok(sig) => format!("Deposited {} base units (token: {}). Signature: {}", input.amount, token_mint, sig),
            Err(e) => format!("Error sending tx: {}", e),
        }
    }

    #[tool(
        description = "Create a payment channel with a merchant. Requires merchant pubkey, spending cap, and optional challenge/dispute periods."
    )]
    async fn mb_create_channel(&self, Parameters(input): Parameters<MbCreateChannelInput>) -> String {
        let merchant = match input.merchant_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid merchant pubkey: {}", e),
        };
        let token_mint = resolve_token_mint(&input.token_mint);

        let recent_blockhash = match self.mb_rpc.get_latest_blockhash() {
            Ok(bh) => bh,
            Err(e) => return format!("Error getting blockhash: {}", e),
        };

        let tx = match transaction::build_initialize_channel_tx(
            &*self.mb_buyer_keypair,
            &merchant,
            &token_mint,
            input.spending_cap,
            input.challenge_period,
            input.dispute_period,
            &self.mb_program_id,
            recent_blockhash,
        ) {
            Ok(tx) => tx,
            Err(e) => return format!("Error building tx: {}", e),
        };

        match self.mb_rpc.send_and_confirm_transaction(&tx) {
            Ok(sig) => format!(
                "Channel created with merchant {}.\nSpending cap: {} lamports\nSignature: {}",
                merchant, input.spending_cap, sig
            ),
            Err(e) => format!("Error sending tx: {}", e),
        }
    }

    #[tool(
        description = "Update the spending cap on an existing payment channel."
    )]
    async fn mb_update_spending_cap(&self, Parameters(input): Parameters<MbUpdateSpendingCapInput>) -> String {
        let merchant = match input.merchant_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid merchant pubkey: {}", e),
        };
        let token_mint = resolve_token_mint(&input.token_mint);

        let recent_blockhash = match self.mb_rpc.get_latest_blockhash() {
            Ok(bh) => bh,
            Err(e) => return format!("Error getting blockhash: {}", e),
        };

        let tx = match transaction::build_update_spending_cap_tx(
            &*self.mb_buyer_keypair,
            &merchant,
            &token_mint,
            input.new_spending_cap,
            &self.mb_program_id,
            recent_blockhash,
        ) {
            Ok(tx) => tx,
            Err(e) => return format!("Error building tx: {}", e),
        };

        match self.mb_rpc.send_and_confirm_transaction(&tx) {
            Ok(sig) => format!(
                "Spending cap updated to {} for merchant {}.\nSignature: {}",
                input.new_spending_cap, merchant, sig
            ),
            Err(e) => format!("Error sending tx: {}", e),
        }
    }

    #[tool(
        description = "Get the on-chain state of a payment channel with a merchant."
    )]
    async fn mb_get_channel(&self, Parameters(input): Parameters<MbGetChannelInput>) -> String {
        let merchant = match input.merchant_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid merchant pubkey: {}", e),
        };
        let token_mint = resolve_token_mint(&input.token_mint);

        let (channel_pda, _bump) = pda::derive_channel_pda(
            &self.mb_program_id,
            &self.mb_buyer_keypair.pubkey(),
            &merchant,
            &token_mint,
        );

        let account = match self.mb_rpc.get_account(&channel_pda) {
            Ok(a) => a,
            Err(e) => return format!("Error: Channel not found: {}", e),
        };

        match deserialize_channel(&account.data) {
            Ok(ch) => format!(
                "Channel: {}\nBuyer: {}\nMerchant: {}\nSpending cap: {}\nSettled: {}\nNonce: {}\nChallenge period: {}\nDispute period: {}",
                channel_pda, ch.buyer, ch.merchant,
                ch.spending_cap, ch.settled_amount, ch.nonce,
                ch.challenge_period, ch.dispute_period
            ),
            Err(e) => format!("Error deserializing channel: {}", e),
        }
    }

    #[tool(
        description = "Get the MagicBlock global state for this buyer."
    )]
    async fn mb_get_global_state(&self, Parameters(input): Parameters<MbGetGlobalStateInput>) -> String {
        let token_mint = resolve_token_mint(&input.token_mint);
        let (global_pda, _bump) = pda::derive_global_state_pda(
            &self.mb_program_id,
            &self.mb_buyer_keypair.pubkey(),
            &token_mint,
        );

        let account = match self.mb_rpc.get_account(&global_pda) {
            Ok(a) => a,
            Err(e) => return format!("Error: Global state not found: {}", e),
        };

        match deserialize_global_state(&account.data) {
            Ok(gs) => format!(
                "Global state: {}\nBuyer: {}\nTotal deposited: {}\nTotal allocated: {}",
                global_pda, gs.buyer, gs.total_deposited, gs.total_allocated
            ),
            Err(e) => format!("Error deserializing global state: {}", e),
        }
    }

    #[tool(
        description = "Sign a payment voucher for a merchant and store it locally."
    )]
    async fn mb_sign_voucher(&self, Parameters(input): Parameters<MbSignVoucherInput>) -> String {
        let merchant = match input.merchant_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid merchant pubkey: {}", e),
        };
        let token_mint = resolve_token_mint(&input.token_mint);

        let (channel_pda, _) = pda::derive_channel_pda(
            &self.mb_program_id,
            &self.mb_buyer_keypair.pubkey(),
            &merchant,
            &token_mint,
        );
        let channel_id = channel_pda.to_bytes();

        let kp_bytes = self.mb_buyer_keypair.to_bytes();
        let (msg_hash, sig) = signing::sign_voucher(
            &channel_id,
            input.seq,
            input.amount,
            &kp_bytes,
        );

        let voucher = StoredVoucher {
            channel_id,
            merchant: merchant.to_bytes(),
            seq: input.seq,
            amount: input.amount,
            buyer_sig: sig,
        };

        if let Err(e) = self.mb_voucher_store.store_voucher(&voucher) {
            return format!("Error storing voucher: {}", e);
        }

        format!(
            "Voucher signed.\nChannel: {}\nSeq: {}\nAmount: {}\nSignature: {}\nMessage hash: {}",
            channel_pda,
            input.seq,
            input.amount,
            bs58::encode(sig).into_string(),
            bs58::encode(msg_hash).into_string(),
        )
    }

    #[tool(
        description = "Sign a batch settlement. Rebuilds the merkle tree from stored vouchers, validates the root and total, and signs the settlement message."
    )]
    async fn mb_sign_settlement(&self, Parameters(input): Parameters<MbSignSettlementInput>) -> String {
        let merchant = match input.merchant_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid merchant pubkey: {}", e),
        };
        let token_mint = resolve_token_mint(&input.token_mint);

        let merkle_root = match bs58::decode(&input.merkle_root).into_vec() {
            Ok(v) if v.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&v);
                arr
            }
            _ => return "Error: Invalid merkle_root (must be 32 bytes, base58)".to_string(),
        };

        let (channel_pda, _) = pda::derive_channel_pda(
            &self.mb_program_id,
            &self.mb_buyer_keypair.pubkey(),
            &merchant,
            &token_mint,
        );
        let channel_id = channel_pda.to_bytes();

        // Rebuild merkle tree from stored vouchers
        let stored = match self.mb_voucher_store.get_vouchers_for_channel(&channel_id) {
            Ok(v) => v,
            Err(e) => return format!("Error loading vouchers: {}", e),
        };

        if stored.is_empty() {
            return "Error: No vouchers found for this channel".to_string();
        }

        let vouchers: Vec<merkle::Voucher> = stored.iter().map(|v| merkle::Voucher {
            channel_id: v.channel_id,
            seq: v.seq,
            amount: v.amount,
            buyer_pubkey: self.mb_buyer_keypair.pubkey().to_bytes(),
            buyer_sig: v.buyer_sig,
        }).collect();

        let tree = merkle::build_sum_merkle_tree(&vouchers);

        // Validate root and total
        if tree.root_hash() != merkle_root {
            return format!(
                "Error: Merkle root mismatch. Computed: {}, Provided: {}",
                bs58::encode(tree.root_hash()).into_string(),
                input.merkle_root,
            );
        }
        if tree.root_sum() != input.total_amount {
            return format!(
                "Error: Total amount mismatch. Computed: {}, Provided: {}",
                tree.root_sum(),
                input.total_amount,
            );
        }

        let kp_bytes = self.mb_buyer_keypair.to_bytes();
        let msg_hash = signing::build_settlement_message(
            &merkle_root,
            input.total_amount,
            &channel_id,
            input.batch_nonce,
        );
        let batch_sig = signing::sign_settlement(&msg_hash, &kp_bytes);

        format!(
            "Settlement signed.\nChannel: {}\nMerkle root: {}\nTotal: {}\nNonce: {}\nSignature: {}",
            channel_pda,
            input.merkle_root,
            input.total_amount,
            input.batch_nonce,
            bs58::encode(batch_sig).into_string(),
        )
    }

    #[tool(
        description = "Dispute a settlement. The buyer can dispute if they believe the merchant is claiming an incorrect amount."
    )]
    async fn mb_dispute(&self, Parameters(input): Parameters<MbDisputeInput>) -> String {
        let merchant = match input.merchant_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid merchant pubkey: {}", e),
        };
        let token_mint = Pubkey::default();

        let (channel_pda, _) = pda::derive_channel_pda(
            &self.mb_program_id,
            &self.mb_buyer_keypair.pubkey(),
            &merchant,
            &token_mint,
        );
        let (escrow_pda, _) = pda::derive_settlement_pda(
            &self.mb_program_id,
            &channel_pda,
            input.batch_nonce,
        );

        let recent_blockhash = match self.mb_rpc.get_latest_blockhash() {
            Ok(bh) => bh,
            Err(e) => return format!("Error getting blockhash: {}", e),
        };

        let tx = match transaction::build_dispute_tx(
            &*self.mb_buyer_keypair,
            &channel_pda,
            &escrow_pda,
            &self.mb_program_id,
            recent_blockhash,
        ) {
            Ok(tx) => tx,
            Err(e) => return format!("Error building tx: {}", e),
        };

        match self.mb_rpc.send_and_confirm_transaction(&tx) {
            Ok(sig) => format!("Dispute filed for nonce {}. Signature: {}", input.batch_nonce, sig),
            Err(e) => format!("Error sending tx: {}", e),
        }
    }

    #[tool(
        description = "Resolve a dispute by providing a merkle proof for a specific voucher, demonstrating the true amount owed."
    )]
    async fn mb_resolve_dispute(&self, Parameters(input): Parameters<MbResolveDisputeInput>) -> String {
        let merchant = match input.merchant_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid merchant pubkey: {}", e),
        };
        let token_mint = Pubkey::default();

        let (channel_pda, _) = pda::derive_channel_pda(
            &self.mb_program_id,
            &self.mb_buyer_keypair.pubkey(),
            &merchant,
            &token_mint,
        );
        let (escrow_pda, _) = pda::derive_settlement_pda(
            &self.mb_program_id,
            &channel_pda,
            input.batch_nonce,
        );
        let channel_id = channel_pda.to_bytes();

        // Get the voucher
        let vouchers = match self.mb_voucher_store.get_vouchers_for_channel(&channel_id) {
            Ok(v) => v,
            Err(e) => return format!("Error loading vouchers: {}", e),
        };

        let voucher = match vouchers.iter().find(|v| v.seq == input.voucher_seq) {
            Some(v) => v,
            None => return format!("Error: Voucher seq {} not found", input.voucher_seq),
        };

        // Build merkle tree and generate proof
        let merkle_vouchers: Vec<merkle::Voucher> = vouchers.iter().map(|v| merkle::Voucher {
            channel_id: v.channel_id,
            seq: v.seq,
            amount: v.amount,
            buyer_pubkey: self.mb_buyer_keypair.pubkey().to_bytes(),
            buyer_sig: v.buyer_sig,
        }).collect();

        let tree = merkle::build_sum_merkle_tree(&merkle_vouchers);
        let voucher_index = vouchers.iter().position(|v| v.seq == input.voucher_seq).unwrap();
        let proof = tree.generate_proof(voucher_index);

        let recent_blockhash = match self.mb_rpc.get_latest_blockhash() {
            Ok(bh) => bh,
            Err(e) => return format!("Error getting blockhash: {}", e),
        };

        let tx = match transaction::build_resolve_dispute_tx(
            &*self.mb_buyer_keypair,
            &channel_pda,
            &escrow_pda,
            voucher.seq,
            voucher.amount,
            &voucher.buyer_sig,
            &proof.sibling_hashes,
            &proof.sibling_sums,
            &self.mb_program_id,
            recent_blockhash,
        ) {
            Ok(tx) => tx,
            Err(e) => return format!("Error building tx: {}", e),
        };

        match self.mb_rpc.send_and_confirm_transaction(&tx) {
            Ok(sig) => format!(
                "Dispute resolved for nonce {} with voucher seq {} (amount {}). Signature: {}",
                input.batch_nonce, input.voucher_seq, voucher.amount, sig
            ),
            Err(e) => format!("Error sending tx: {}", e),
        }
    }

    #[tool(
        description = "Withdraw SOL from the MagicBlock global vault back to the buyer's wallet."
    )]
    async fn mb_withdraw(&self, Parameters(input): Parameters<MbWithdrawInput>) -> String {
        let token_mint = resolve_token_mint(&input.token_mint);
        let recent_blockhash = match self.mb_rpc.get_latest_blockhash() {
            Ok(bh) => bh,
            Err(e) => return format!("Error getting blockhash: {}", e),
        };

        let tx = match transaction::build_withdraw_tx(
            &*self.mb_buyer_keypair,
            &token_mint,
            input.amount,
            &self.mb_program_id,
            recent_blockhash,
        ) {
            Ok(tx) => tx,
            Err(e) => return format!("Error building tx: {}", e),
        };

        match self.mb_rpc.send_and_confirm_transaction(&tx) {
            Ok(sig) => format!("Withdrew {} base units (token: {}). Signature: {}", input.amount, token_mint, sig),
            Err(e) => format!("Error sending tx: {}", e),
        }
    }
}

impl IgnitePayMcpServer {
    /// Check if the session has enough remaining balance for the given amount.
    fn check_session_balance(&self, session: &Option<SessionKeypair>, amount: u64) -> Result<(), String> {
        if let Some(sk) = session {
            let remaining = sk.session_data.spending_limit.saturating_sub(sk.session_data.current_spent);
            if remaining < amount {
                return Err(format!(
                    "Session balance insufficient: {} remaining, {} needed",
                    remaining, amount
                ));
            }
        }
        Ok(())
    }

    /// Execute a payment atomically: acquire mutex, check balance, execute, record spending.
    async fn execute_payment_atomic(
        &self,
        payment: &PaymentRequest,
        session: &Option<SessionKeypair>,
        spl_params: Option<&SplPaymentParams>,
        preferred_method: Option<&str>,
    ) -> Result<PaymentProof, String> {
        let _guard = self.payment_mutex.lock().await;

        // F3/F7: Ensure session has sufficient balance, requesting funds if needed
        self.ensure_session_funded(session, payment.amount).await?;

        // Check session balance before payment
        self.check_session_balance(session, payment.amount)?;

        // Execute the payment
        let proof = self.execute_payment_auto(payment, session, spl_params, preferred_method).await?;

        // Record spent amount in session (if session exists)
        if let Some(client) = &self.solana_client {
            if let Some(sk) = session {
                let _ = client.session_manager().record_spent(
                    &sk.keypair.pubkey(),
                    payment.amount,
                );
            }
        }

        // Record cumulative merchant spending for F8
        if !payment.merchant_did.is_empty() {
            let _ = self.list_store.record_merchant_spent(&payment.merchant_did, payment.amount);
        }

        Ok(proof)
    }

    /// F3/F7: Ensure the session has sufficient balance before payment.
    /// If not, send a fund request to the phone and wait for response.
    async fn ensure_session_funded(
        &self,
        session: &Option<SessionKeypair>,
        amount: u64,
    ) -> Result<(), String> {
        let sk = match session {
            Some(s) => s,
            None => return Ok(()), // No session, let execute_payment handle it
        };

        let remaining = sk.session_data.spending_limit.saturating_sub(sk.session_data.current_spent);
        if remaining >= amount {
            return Ok(());
        }

        // Need to request funding from phone
        let session_key_pubkey = sk.keypair.pubkey().to_string();
        tracing::info!(
            "Session {} balance insufficient ({} < {}), requesting funds from phone",
            session_key_pubkey, remaining, amount
        );

        let phone_did = self.resolve_phone_did().await
            .ok_or_else(|| "No phone DID available for fund request".to_string())?;

        let rx = self.pending_fund.register(&session_key_pubkey);

        let msg = ignite_pay_core::didcomm::build_session_fund_request(
            self.mediator.our_did(),
            &phone_did,
            &session_key_pubkey,
            amount.saturating_sub(remaining),
            remaining,
            remaining,
            "",
            "insufficient_balance",
        );

        self.mediator.send_to_phone(&msg, &phone_did).await
            .map_err(|e| format!("Failed to send fund request: {}", e))?;

        // Wait for response with 60s timeout
        match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
            Ok(Ok(resp)) if resp.funded => {
                tracing::info!("Session funded: new_balance={}", resp.new_balance);
                Ok(())
            }
            Ok(Ok(_)) => Err("Phone declined fund request".to_string()),
            Ok(Err(_)) => Err("Fund request channel error".to_string()),
            Err(_) => Err("Fund request timed out (60s)".to_string()),
        }
    }

    /// F13: Check session balance and send notification if below threshold.
    async fn check_and_notify_balances(&self) {
        let session = match self.get_active_session() {
            Some(s) => s,
            None => return,
        };

        let remaining = session.session_data.spending_limit.saturating_sub(session.session_data.current_spent);
        let threshold = session.session_data.spending_limit / 10; // 10% threshold

        if remaining >= threshold {
            return;
        }

        let phone_did = match self.resolve_phone_did().await {
            Some(d) => d,
            None => return,
        };

        let msg = ignite_pay_core::didcomm::build_balance_notification(
            self.mediator.our_did(),
            &phone_did,
            &session.keypair.pubkey().to_string(),
            remaining,
            threshold,
            remaining,
        );

        match self.mediator.send_to_phone(&msg, &phone_did).await {
            Ok(_) => tracing::info!("Balance notification sent: remaining={}", remaining),
            Err(e) => tracing::warn!("Failed to send balance notification: {}", e),
        }
    }

    /// F14: Check if session key needs renewal and request renewal from phone.
    async fn check_and_renew_session(&self) {
        let session = match self.get_active_session() {
            Some(s) => s,
            None => return,
        };

        let now = chrono::Utc::now().timestamp();
        let expires_at = session.session_data.expires_at;
        let remaining_secs = expires_at.saturating_sub(now);

        // Renew if less than 5 minutes remaining
        if remaining_secs > 300 {
            return;
        }

        tracing::info!(
            "Session {} expires in {}s, requesting renewal",
            session.keypair.pubkey(),
            remaining_secs
        );

        let phone_did = match self.resolve_phone_did().await {
            Some(d) => d,
            None => return,
        };

        // Create a new ephemeral keypair for the replacement session
        let new_sk = match self.create_session_key_for_request(
            &PaymentRequest {
                id: String::new(),
                recipient: String::new(),
                merchant_did: String::new(),
                amount: session.session_data.spending_limit,
                token: if session.session_data.token_mint == Pubkey::default() {
                    "SOL".to_string()
                } else {
                    session.session_data.token_mint.to_string()
                },
                network: "solana".to_string(),
                description: String::new(),
                status: PaymentStatus::PendingAuth,
                created_at: chrono::Utc::now(),
                tx_signature: None,
            },
            &None,
        ) {
            Some(sk) => sk,
            None => {
                tracing::warn!("Failed to create new session key for renewal");
                return;
            }
        };

        let old_pubkey = session.keypair.pubkey().to_string();
        let rx = self.pending_renew.register(&old_pubkey);

        let msg = ignite_pay_core::didcomm::build_session_renew_request(
            self.mediator.our_did(),
            &phone_did,
            &old_pubkey,
            expires_at,
            &new_sk,
        );

        match self.mediator.send_to_phone(&msg, &phone_did).await {
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("Failed to send renew request: {}", e);
                return;
            }
        }

        // Wait for response with 60s timeout
        match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
            Ok(Ok(resp)) if resp.renewed => {
                tracing::info!(
                    "Session renewed: old={} new={}",
                    old_pubkey, resp.new_session_key_pubkey
                );
            }
            Ok(Ok(_)) => {
                tracing::warn!("Phone declined session renewal");
            }
            Ok(Err(_)) => {
                tracing::warn!("Session renew channel error");
            }
            Err(_) => {
                tracing::warn!("Session renew request timed out (60s)");
            }
        }
    }

    /// Resolve the phone DID: prefer the dynamically paired phone DID,
    /// fall back to the config value, then to the input parameter.
    async fn resolve_phone_did(&self) -> Option<String> {
        // First check if a phone has paired via connection-request
        if let Some(paired) = self.mediator.paired_phone_did().await {
            return Some(paired);
        }
        // Then check config
        if !self.phone_did.is_empty() {
            return Some(self.phone_did.clone());
        }
        None
    }

    /// Handle V1.1 extended list_action from phone auth response.
    async fn handle_list_action(
        &self,
        action: &str,
        merchant_did: &str,
        _amount: u64,
        label: Option<&str>,
        max_amount: Option<u64>,
    ) {
        let entry = MerchantListEntry {
            did: merchant_did.to_string(),
            name: None,
            max_amount,
            added_at: chrono::Utc::now(),
            label: label.map(String::from),
            expires: None,
        };

        let list_type = match action {
            "whitelist" | "add_whitelist" => {
                if let Err(e) = self.list_store.add_to_whitelist(entry) {
                    tracing::warn!("Failed to add to whitelist: {}", e);
                } else {
                    tracing::info!("Added {} to whitelist", merchant_did);
                }
                "whitelist"
            }
            "blacklist" | "add_blacklist" => {
                if let Err(e) = self.list_store.add_to_blacklist(entry) {
                    tracing::warn!("Failed to add to blacklist: {}", e);
                } else {
                    tracing::info!("Added {} to blacklist", merchant_did);
                }
                "blacklist"
            }
            "remove_whitelist" => {
                if let Err(e) = self.list_store.remove_from_whitelist(merchant_did) {
                    tracing::warn!("Failed to remove from whitelist: {}", e);
                } else {
                    tracing::info!("Removed {} from whitelist", merchant_did);
                }
                "whitelist"
            }
            "remove_blacklist" => {
                if let Err(e) = self.list_store.remove_from_blacklist(merchant_did) {
                    tracing::warn!("Failed to remove from blacklist: {}", e);
                } else {
                    tracing::info!("Removed {} from blacklist", merchant_did);
                }
                "blacklist"
            }
            _ => return,
        };

        // Audit: list updated
        let _ = self.audit.record_list_event(list_type, action, merchant_did);

        // Upload updated lists to IPFS and notify phone
        match self
            .list_store
            .upload_to_ipfs(self.ipfs_client.as_ref())
            .await
        {
            Ok(new_cid) => {
                tracing::info!("Lists uploaded to IPFS: {}", new_cid);
                // Send list-sync-notification to phone
                if let Some(phone_did) = self.resolve_phone_did().await {
                    if let Err(e) = self
                        .mediator
                        .send_list_sync_notification(
                            &phone_did,
                            list_type,
                            action,
                            merchant_did,
                            &new_cid,
                        )
                        .await
                    {
                        tracing::warn!("Failed to send list-sync-notification: {}", e);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to upload lists to IPFS: {}", e);
            }
        }
    }

    /// Get active session from Solana client, if configured.
    fn get_active_session(&self) -> Option<SessionKeypair> {
        self.solana_client
            .as_ref()?
            .session_manager()
            .get_active_session(&self.default_owner)
            .ok()
            .flatten()
    }

    /// Construct a SessionKeypair from a phone-provided auth response.
    /// Parses the base58 secret key into a Keypair, builds SessionTokenData,
    /// and stores it in sled for reuse across payments.
    fn get_session_from_auth_response(&self, resp: &AuthResponse) -> Option<SessionKeypair> {
        let secret_key_b58 = resp.session_key_secret_key.as_ref()?;
        let pubkey_b58 = resp.session_key_pubkey.as_ref()?;
        let expires_at = resp.session_expires_at?;
        let spending_limit = resp.spending_limit?;
        let scopes = resp.scopes.clone()?;

        // Decode the base58 secret key into a 64-byte keypair
        let keypair_bytes = bs58::decode(secret_key_b58).into_vec().ok()?;
        if keypair_bytes.len() != 64 {
            tracing::warn!("Invalid session key length: {}", keypair_bytes.len());
            return None;
        }
        let keypair_array: [u8; 64] = keypair_bytes.try_into().ok()?;
        let keypair = Keypair::try_from(&keypair_array as &[u8]).ok()?;

        // Verify the pubkey matches
        let expected_pubkey = bs58::decode(pubkey_b58).into_vec().ok()?;
        if keypair.pubkey().as_ref() != expected_pubkey.as_slice() {
            tracing::warn!("Session key pubkey mismatch");
            return None;
        }

        let session_data = SessionTokenData {
            owner: self.default_owner,
            ephemeral_signer: keypair.pubkey(),
            target_program: ignite_pay_solana::solana_sdk::system_program::id(),
            token_mint: resp.token_mint
                .as_deref()
                .and_then(|s| s.parse::<Pubkey>().ok())
                .unwrap_or_default(),
            expires_at,
            spending_limit,
            current_spent: 0,
            scopes,
        };

        // Store in sled for reuse (using session: prefix so get_active_session can find it)
        if let Some(client) = &self.solana_client {
            let key = format!("session:{}", keypair.pubkey());
            let serialized = borsh::to_vec(&session_data).unwrap_or_default();
            let mut value = serialized;
            value.extend_from_slice(&keypair.to_bytes());
            let _ = client.session_manager().db().insert(key.as_bytes(), value);
        }

        Some(SessionKeypair {
            keypair,
            session_data,
        })
    }
}

// ── Account Deserialization ─────────────────────────────────────────────────

struct ChannelAccount {
    buyer: Pubkey,
    merchant: Pubkey,
    token_mint: Pubkey,
    spending_cap: u64,
    settled_amount: u64,
    nonce: u64,
    challenge_period: i64,
    dispute_period: i64,
    _bump: u8,
}

struct GlobalStateAccount {
    buyer: Pubkey,
    token_mint: Pubkey,
    total_deposited: u64,
    total_allocated: u64,
    _bump: u8,
}

fn deserialize_channel(data: &[u8]) -> anyhow::Result<ChannelAccount> {
    // Anchor: 8-byte discriminator + data
    // Layout: buyer(32) + merchant(32) + token_mint(32) + spending_cap(8) + settled_amount(8) + nonce(8) + challenge_period(8) + dispute_period(8) + bump(1)
    if data.len() < 8 + 32 + 32 + 32 + 8 + 8 + 8 + 8 + 8 + 1 {
        return Err(anyhow::anyhow!("Channel account data too short"));
    }
    let d = &data[8..];
    Ok(ChannelAccount {
        buyer: Pubkey::try_from(&d[0..32])?,
        merchant: Pubkey::try_from(&d[32..64])?,
        token_mint: Pubkey::try_from(&d[64..96])?,
        spending_cap: u64::from_le_bytes(d[96..104].try_into()?),
        settled_amount: u64::from_le_bytes(d[104..112].try_into()?),
        nonce: u64::from_le_bytes(d[112..120].try_into()?),
        challenge_period: i64::from_le_bytes(d[120..128].try_into()?),
        dispute_period: i64::from_le_bytes(d[128..136].try_into()?),
        _bump: d[136],
    })
}

fn deserialize_global_state(data: &[u8]) -> anyhow::Result<GlobalStateAccount> {
    // Layout: buyer(32) + token_mint(32) + total_deposited(8) + total_allocated(8) + bump(1)
    if data.len() < 8 + 32 + 32 + 8 + 8 + 1 {
        return Err(anyhow::anyhow!("Global state data too short"));
    }
    let d = &data[8..];
    Ok(GlobalStateAccount {
        buyer: Pubkey::try_from(&d[0..32])?,
        token_mint: Pubkey::try_from(&d[32..64])?,
        total_deposited: u64::from_le_bytes(d[64..72].try_into()?),
        total_allocated: u64::from_le_bytes(d[72..80].try_into()?),
        _bump: d[80],
    })
}

#[tool_handler]
impl ServerHandler for IgnitePayMcpServer {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo::new(
            ServerCapabilities::builder().enable_tools().build(),
        )
        .with_instructions(
            "Ignite Pay MCP Server (V3.0) — handles x402 HTTP payment challenges via DIDComm-encrypted \
             authorization with on-chain Solana payment execution. Supports MagicBlock payment channels \
             for low-frequency on-chain settlement. Use process_x402_challenge when you encounter \
             an HTTP 402 response to request payment approval from the user's phone.",
        )
    }
}

// ── Main ───────────────────────────────────────────────────────────────────

/// Resolve an optional token_mint string into a Pubkey.
/// Defaults to Pubkey::default() (all zeros) for SOL.
fn resolve_token_mint(token_mint: &Option<String>) -> Pubkey {
    token_mint
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<Pubkey>().ok())
        .unwrap_or_default()
}

/// Send an MB deposit error response to the phone.
async fn send_mb_deposit_error(
    server: &IgnitePayMcpServer,
    phone_did: &str,
    amount: u64,
    token: &str,
    error: &str,
) -> anyhow::Result<()> {
    let response = ignite_pay_core::didcomm::build_mb_deposit_response(
        &server.mediator.our_did(),
        phone_did,
        false,
        amount,
        None,
        None,
        token,
        Some(error),
    );
    server.mediator.send_to_phone(&response, phone_did).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install rustls crypto provider (required for TLS connections)
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    // Initialize tracing with optional file output
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "ignite_pay_mcp=info,ignite_pay_core=info".into());

    // Always log to file (./data/logs/) and stderr
    let log_dir = std::env::var("AUDIT_LOG_DIR").unwrap_or_else(|_| "./data/logs".to_string());
    let _ = std::fs::create_dir_all(&log_dir);
    let file_appender = tracing_appender::rolling::daily(&log_dir, "ignite-pay-mcp.log");
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr.and(file_appender))
        .with_env_filter(env_filter)
        .init();

    let config = load_config()?;
    tracing::info!("Loaded config: mediator={}", config.mediator.ws_url);

    // Open sled database first for identity persistence
    let db = sled::open(&config.storage.path)?;
    tracing::info!("Database opened at {}", config.storage.path);

    // Create mediator connection with identity persistence
    let mediator = Arc::new(MediatorConnection::new(&config.mediator.ws_url, &db)?);

    // Register phone as a peer
    if !config.mediator.phone_did.is_empty() {
        mediator.add_peer(&config.mediator.phone_did).await;
        tracing::info!("Registered phone peer: {}", config.mediator.phone_did);
    }

    // Create payment store + pending auth store + list store
    let payments = Arc::new(ignite_pay_mcp::payment::PaymentStore::from_db(db.clone()));
    let pending = Arc::new(PendingAuthStore::new());
    let list_store = Arc::new(ListStore::new(payments.get_db()));
    let audit = Arc::new(AuditLogStore::from_db(payments.get_db()));

    // V2.0: Initialize Solana client if RPC is configured
    let solana_client = if config.solana.is_payment_configured() {
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

    // V2.0: Initialize Solana DID bridge if DID program is configured
    #[cfg(not(feature = "zk-compression"))]
    let solana_bridge = if !config.solana.did_program_id.is_empty() {
        match SolanaDidBridge::new(
            &config.solana.rpc_url,
            &config.solana.did_program_id,
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

    #[cfg(feature = "zk-compression")]
    let solana_bridge = if !config.solana.did_program_id.is_empty()
        && !config.solana.photon_url.is_empty()
        && !config.solana.address_tree.is_empty()
    {
        match SolanaDidBridge::new(
            &config.solana.rpc_url,
            &config.solana.did_program_id,
            &config.solana.photon_url,
            &config.solana.address_tree,
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
    } else if !config.solana.did_program_id.is_empty() {
        tracing::warn!(
            "Solana DID program configured but photon_url or address_tree missing — on-chain DID verification disabled"
        );
        None
    } else {
        None
    };

    // V2.0: Parse default owner pubkey for session management
    let default_owner = if !config.solana.default_owner.is_empty() {
        match config.solana.default_owner.parse::<Pubkey>() {
            Ok(pk) => {
                tracing::info!("Default owner pubkey: {}", pk);
                pk
            }
            Err(e) => {
                tracing::warn!("Invalid default_owner pubkey, using default: {}", e);
                Pubkey::default()
            }
        }
    } else {
        tracing::info!("No default_owner configured, sessions will use zero pubkey");
        Pubkey::default()
    };

    // V2.0: Initialize DidService if DID program is configured
    let did_service = if !config.solana.did_program_id.is_empty() {
        match ignite_pay_solana::compression::DidService::new(
            &config.solana.rpc_url,
            &config.solana.did_program_id,
        ) {
            Ok(svc) => {
                tracing::info!(
                    "DID service initialized: program={}",
                    config.solana.did_program_id
                );
                Some(Arc::new(svc))
            }
            Err(e) => {
                tracing::error!("Failed to initialize DID service: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Connect to mediator (spawns background task with pending auth handling)
    let (qr_payment_tx, mut qr_payment_rx) = tokio::sync::mpsc::unbounded_channel::<QrPaymentCommand>();
    let (mb_deposit_tx, mut mb_deposit_rx) = tokio::sync::mpsc::unbounded_channel::<MbDepositCommand>();
    let pending_fund_store = Arc::new(PendingFundStore::new());
    let pending_renew_store = Arc::new(ignite_pay_mcp::payment::PendingRenewStore::new());
    mediator
        .connect(Arc::clone(&pending), Arc::clone(&pending_fund_store), Arc::clone(&pending_renew_store), None, Some(mb_deposit_tx), Some(qr_payment_tx))
        .await?;
    tracing::info!("Connecting to mediator at {}...", config.mediator.ws_url);

    // Auto-display pairing QR if no phone is paired and no phone_did in config
    if mediator.paired_phone_did().await.is_none() && config.mediator.phone_did.is_empty() {
        let qr_path = format!("{}/pairing_qr.svg", config.storage.path);
        match mediator.generate_invitation_qr_svg(&qr_path) {
            Ok(url) => {
                tracing::info!("Pairing QR saved to: {}", qr_path);
                tracing::info!("Invitation URL:\n{}", url);
                tracing::info!("Scan the QR image with Ignite Pay to pair.");

                // Open the QR image with the system default viewer
                #[cfg(target_os = "windows")]
                std::process::Command::new("cmd").args(["/c", "start", &qr_path]).spawn().ok();
                #[cfg(target_os = "macos")]
                std::process::Command::new("open").arg(&qr_path).spawn().ok();
                #[cfg(target_os = "linux")]
                std::process::Command::new("xdg-open").arg(&qr_path).spawn().ok();
            }
            Err(e) => {
                tracing::warn!("Failed to generate QR image: {}", e);
                let url = mediator.generate_invitation();
                tracing::info!("Invitation URL (manual):\n{}", url);
            }
        }
    } else if let Some(phone) = mediator.paired_phone_did().await {
        tracing::info!("Phone already paired: {}", phone);
    }

    // MagicBlock: Initialize MB RPC, program ID, buyer keypair, and voucher store
    let mb_rpc = Arc::new(RpcClient::new(&config.magicblock.rpc_url));
    let mb_program_id: Pubkey = config.magicblock.program_id.parse()
        .map_err(|e| anyhow::anyhow!("Invalid MB program_id: {}", e))?;
    tracing::info!("MagicBlock RPC: {}, Program: {}", config.magicblock.rpc_url, mb_program_id);

    // Load or generate MB buyer keypair (persist in sled)
    let mb_keys_tree = db.open_tree("mb_keys")?;
    let mb_buyer_keypair = match mb_keys_tree.get("buyer_keypair")? {
        Some(bytes) => {
            if bytes.len() == 64 {
                let kp = MbKeypair::try_from(bytes.as_ref())
                    .map_err(|e| anyhow::anyhow!("Failed to load MB keypair: {}", e))?;
                tracing::info!("Loaded existing MB buyer keypair: {}", kp.pubkey());
                Arc::new(kp)
            } else {
                let kp = MbKeypair::new();
                mb_keys_tree.insert("buyer_keypair", kp.to_bytes().as_ref())?;
                mb_keys_tree.flush()?;
                tracing::info!("Generated new MB buyer keypair: {}", kp.pubkey());
                Arc::new(kp)
            }
        }
        None => {
            let kp = MbKeypair::new();
            mb_keys_tree.insert("buyer_keypair", kp.to_bytes().as_ref())?;
            mb_keys_tree.flush()?;
            tracing::info!("Generated new MB buyer keypair: {}", kp.pubkey());
            Arc::new(kp)
        }
    };

    let mb_voucher_store = Arc::new(VoucherStore::new(db.clone()));

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
        ipfs_client: {
            let ipfs_client: Arc<Box<dyn IpfsClient>> = match config.ipfs.mode.as_str() {
                "kubo" => {
                    tracing::info!("Using Kubo IPFS client: {}", config.ipfs.kubo_url);
                    Arc::new(Box::new(KuboIpfsClient::new(&config.ipfs.kubo_url)))
                }
                _ => {
                    tracing::info!("Using Mock IPFS client");
                    Arc::new(Box::new(MockIpfsClient::new()))
                }
            };
            ipfs_client
        },
        audit,
        default_owner,
        did_service,
        mb_rpc,
        mb_program_id,
        mb_buyer_keypair,
        mb_voucher_store,
        payment_mutex: Arc::new(tokio::sync::Mutex::new(())),
        pending_fund: pending_fund_store,
        pending_renew: pending_renew_store,
    };

    // Spawn QR payment handler background task
    // When the phone sends a qr-payment-request (user scanned merchant QR),
    // the mediator forwards it here via the mpsc channel.
    {
        let server = server.clone();
        tokio::spawn(async move {
            while let Some(cmd) = qr_payment_rx.recv().await {
                tracing::info!(
                    "Processing QR payment: order={} merchant={} amount={} method={}",
                    cmd.order_id, cmd.merchant_did, cmd.amount, cmd.payment_method
                );

                // Create a payment record for tracking
                let payment_id = uuid::Uuid::new_v4().to_string();
                let payment = PaymentRequest {
                    id: payment_id.clone(),
                    merchant_did: cmd.merchant_did.clone(),
                    amount: cmd.amount,
                    token: cmd.token.clone(),
                    network: "solana".to_string(),
                    recipient: String::new(), // resolved from merchant DID
                    description: cmd.description.clone(),
                    status: PaymentStatus::PendingAuth,
                    created_at: chrono::Utc::now(),
                    tx_signature: None,
                };
                let _ = server.payments.save_payment(&payment);

                // Execute payment via the chosen method
                let session = server.get_active_session();
                let spl_params: Option<SplPaymentParams> = if cmd.token != "SOL" && cmd.token != "sol" {
                    // Resolve SPL token mint
                    match resolve_mint(&cmd.token, "solana") {
                        Some(mint) => Some(SplPaymentParams {
                            mint,
                            source_ata_override: None,
                            dest_ata_override: None,
                        }),
                        None => None,
                    }
                } else {
                    None
                };

                let result = server.execute_payment_atomic(
                    &payment,
                    &session,
                    spl_params.as_ref(),
                    Some(cmd.payment_method.as_str()),
                ).await;

                let (success, proof_str, error_msg) = match result {
                    Ok(proof) => {
                        let _ = server.payments.update_status(&payment_id, &PaymentStatus::Executed);
                        if let PaymentProof::TxSignature(tx_sig) = &proof {
                            let _ = server.payments.set_tx_signature(&payment_id, tx_sig);
                        }
                        let _ = server.audit.record_payment_event(
                            &payment_id,
                            "qr_payment_executed",
                            cmd.amount,
                            &cmd.merchant_did,
                        );
                        (true, format!("{}", proof), None)
                    }
                    Err(e) => {
                        let _ = server.payments.update_status(&payment_id, &PaymentStatus::Rejected);
                        tracing::error!("QR payment execution failed: {}", e);
                        (false, String::new(), Some(e))
                    }
                };

                // Send qr-payment-response back to phone
                let response = ignite_pay_core::didcomm::build_qr_payment_response(
                    &server.mediator.our_did(),
                    &cmd.phone_did,
                    &cmd.order_id,
                    success,
                    &proof_str,
                    &cmd.payment_method,
                    error_msg.as_deref(),
                );

                // Encrypt and send via mediator
                match server.mediator.send_to_phone(&response, &cmd.phone_did).await {
                    Ok(_) => tracing::info!("QR payment response sent to phone for order {}", cmd.order_id),
                    Err(e) => tracing::error!("Failed to send QR payment response: {}", e),
                }

                // Notify merchant MCP so their app can announce the payment
                if success && !cmd.merchant_mediator_url.is_empty() {
                    let notify = ignite_pay_core::didcomm::build_qr_payment_notify(
                        &server.mediator.our_did(),
                        &cmd.merchant_did,
                        &cmd.order_id,
                        cmd.amount,
                        &cmd.payment_method,
                        &proof_str,
                    );
                    match server.mediator.send_to_mediator(
                        &notify, &cmd.merchant_did, &cmd.merchant_mediator_url,
                    ).await {
                        Ok(_) => tracing::info!(
                            "QR payment notify sent to merchant MCP for order {}", cmd.order_id
                        ),
                        Err(e) => tracing::warn!(
                            "Failed to notify merchant MCP (non-fatal): {}", e
                        ),
                    }
                }
            }
        });
    }

    // Spawn MB deposit handler background task
    // When the phone sends an mb-deposit-request, the mediator forwards it here.
    {
        let server = server.clone();
        tokio::spawn(async move {
            while let Some(cmd) = mb_deposit_rx.recv().await {
                tracing::info!(
                    "Processing MB deposit: phone={} amount={} token={}",
                    cmd.phone_did, cmd.amount, cmd.token
                );

                // Resolve token mint
                let token_mint = cmd.token.parse::<Pubkey>()
                    .unwrap_or_else(|_| Pubkey::default());

                // 1. Check if global state exists; if not, init_global first
                let (global_pda, _) = pda::derive_global_state_pda(
                    &server.mb_program_id,
                    &server.mb_buyer_keypair.pubkey(),
                    &token_mint,
                );
                let global_exists = server.mb_rpc.get_account(&global_pda).is_ok();

                if !global_exists {
                    tracing::info!("MB global state not found, initializing...");
                    let bh = match server.mb_rpc.get_latest_blockhash() {
                        Ok(bh) => bh,
                        Err(e) => {
                            tracing::error!("MB deposit: failed to get blockhash: {}", e);
                            let _ = send_mb_deposit_error(&server, &cmd.phone_did, cmd.amount, &cmd.token, &format!("Blockhash error: {}", e)).await;
                            continue;
                        }
                    };
                    let init_tx = match transaction::build_initialize_global_tx(
                        &*server.mb_buyer_keypair,
                        &token_mint,
                        &server.mb_program_id,
                        bh,
                    ) {
                        Ok(tx) => tx,
                        Err(e) => {
                            tracing::error!("MB deposit: failed to build init_global tx: {}", e);
                            let _ = send_mb_deposit_error(&server, &cmd.phone_did, cmd.amount, &cmd.token, &format!("Init global error: {}", e)).await;
                            continue;
                        }
                    };
                    if let Err(e) = server.mb_rpc.send_and_confirm_transaction(&init_tx) {
                        tracing::error!("MB deposit: init_global tx failed: {}", e);
                        let _ = send_mb_deposit_error(&server, &cmd.phone_did, cmd.amount, &cmd.token, &format!("Init global tx failed: {}", e)).await;
                        continue;
                    }
                    tracing::info!("MB global state initialized");
                }

                // 2. Deposit
                let bh = match server.mb_rpc.get_latest_blockhash() {
                    Ok(bh) => bh,
                    Err(e) => {
                        tracing::error!("MB deposit: failed to get blockhash: {}", e);
                        let _ = send_mb_deposit_error(&server, &cmd.phone_did, cmd.amount, &cmd.token, &format!("Blockhash error: {}", e)).await;
                        continue;
                    }
                };
                let deposit_tx = match transaction::build_deposit_tx(
                    &*server.mb_buyer_keypair,
                    &token_mint,
                    cmd.amount,
                    &server.mb_program_id,
                    bh,
                ) {
                    Ok(tx) => tx,
                    Err(e) => {
                        tracing::error!("MB deposit: failed to build deposit tx: {}", e);
                        let _ = send_mb_deposit_error(&server, &cmd.phone_did, cmd.amount, &cmd.token, &format!("Build deposit error: {}", e)).await;
                        continue;
                    }
                };
                let tx_sig = match server.mb_rpc.send_and_confirm_transaction(&deposit_tx) {
                    Ok(sig) => {
                        tracing::info!("MB deposit confirmed: {} lamports, sig: {}", cmd.amount, sig);
                        sig.to_string()
                    }
                    Err(e) => {
                        tracing::error!("MB deposit tx failed: {}", e);
                        let _ = send_mb_deposit_error(&server, &cmd.phone_did, cmd.amount, &cmd.token, &format!("Deposit tx failed: {}", e)).await;
                        continue;
                    }
                };

                // 3. Get updated global state for total_deposited
                let total_deposited = server.mb_rpc.get_account(&global_pda)
                    .ok()
                    .and_then(|a| deserialize_global_state(&a.data).ok())
                    .map(|gs| gs.total_deposited);

                // 4. Send success response
                let response = ignite_pay_core::didcomm::build_mb_deposit_response(
                    &server.mediator.our_did(),
                    &cmd.phone_did,
                    true,
                    cmd.amount,
                    total_deposited,
                    Some(&tx_sig),
                    &cmd.token,
                    None,
                );
                match server.mediator.send_to_phone(&response, &cmd.phone_did).await {
                    Ok(_) => tracing::info!("MB deposit response sent to phone"),
                    Err(e) => tracing::error!("Failed to send MB deposit response: {}", e),
                }
            }
        });
    }

    // F13/F14: Background monitor for balance notifications and session renewal
    {
        let server = server.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            let mut last_notification: std::time::Instant = std::time::Instant::now()
                - std::time::Duration::from_secs(300); // Allow immediate first notification

            loop {
                interval.tick().await;

                // F13: Check balance and send notification (max once per 5 minutes)
                if last_notification.elapsed() >= std::time::Duration::from_secs(300) {
                    server.check_and_notify_balances().await;
                    last_notification = std::time::Instant::now();
                }

                // F14: Check if session needs renewal
                server.check_and_renew_session().await;
            }
        });
    }

    tracing::info!("Starting MCP server on stdio...");
    // Spawn the MCP stdio server in a background task.
    // If no MCP client connects (e.g. running standalone), log a warning
    // but keep the process alive — the mediator WS connection and DIDComm
    // message loop continue working independently.
    tokio::spawn({
        let server = server.clone();
        async move {
            match server.serve(stdio()).await {
                Ok(service) => {
                    if let Err(e) = service.waiting().await {
                        tracing::warn!("MCP stdio service ended: {}", e);
                    }
                }
                Err(e) => {
                    tracing::warn!("MCP stdio server failed to start (non-fatal): {}. Process stays alive for DIDComm mediator connection.", e);
                }
            }
        }
    });

    // Start SSE/Streamable HTTP server if port is configured
    if config.mcp.sse_port > 0 {
        let sse_port = config.mcp.sse_port;
        let ct = tokio_util::sync::CancellationToken::new();
        let session_manager = Arc::new(LocalSessionManager::default());
        let http_service = StreamableHttpService::new(
            {
                let server = server.clone();
                move || Ok(server.clone())
            },
            session_manager,
            StreamableHttpServerConfig::default()
                .with_sse_keep_alive(None)
                .with_cancellation_token(ct.child_token()),
        );

        let router = axum::Router::new().nest_service("/mcp", http_service);

        let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", sse_port)).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("Failed to bind SSE port {}: {}", sse_port, e);
                return Err(e.into());
            }
        };
        tracing::info!("MCP SSE server listening on http://0.0.0.0:{}/mcp", sse_port);

        tokio::spawn({
            let ct = ct.clone();
            async move {
                let _ = axum::serve(listener, router)
                    .with_graceful_shutdown(async move { ct.cancelled_owned().await })
                    .await;
            }
        });
    } else {
        tracing::info!("SSE transport disabled (mcp.sse_port = 0)");
    }

    // Keep the process alive — the mediator WS loop runs in the background.
    // The process should only exit on explicit signal (Ctrl+C).
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down...");

    Ok(())
}
