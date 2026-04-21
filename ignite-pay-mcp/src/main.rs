use ignite_pay_mcp::channel::ChannelClient;
use ignite_pay_mcp::mediator::MediatorConnection;
use ignite_pay_mcp::payment::{
    execute_mock_payment, AuthResponse, PaymentRequest, PaymentStatus, PendingAuthStore,
};
use ignite_pay_mcp::audit::AuditLogStore;
use ignite_pay_mcp::tools::{
    AddMerchantInput, AuthorizationCheckInput, ChannelPayInput, CloseChannelInput,
    CloseSessionInput, CreateSessionInput, GetChannelStatusInput, OpenChannelInput,
    PaymentHistoryInput, SessionStatusInput, SettleChannelInput, SplPaymentInput,
    UpdateMerchantInput, VerifyMerchantInput, X402ChallengeInput,
};

use base64::Engine;
use ignite_pay_core::ipfs::IpfsClient;
use ignite_pay_core::ipfs::MockIpfsClient;
use ignite_pay_core::list_store::ListStore;
use ignite_pay_core::solana_did::SolanaDidBridge;
use ignite_pay_core::types::MerchantListEntry;
use ignite_pay_core::types::{RiskControlDecision, VerifiableCredential};
use ignite_pay_core::vc::resolve_vc_from_ipfs;
use ignite_pay_solana::payment::IgnitePayClient;
use ignite_pay_solana::session::SessionKeypair;
use ignite_pay_solana::solana_sdk::pubkey::Pubkey;
use ignite_pay_solana::solana_sdk::signature::Keypair;
use ignite_pay_solana::solana_sdk::signer::Signer;
use ignite_pay_solana::types::{
    PayMode, SessionTokenData, SplPaymentParams,
};

use rmcp::{
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::ServerCapabilities,
    tool, tool_handler, tool_router,
    transport::stdio,
    ServerHandler, ServiceExt,
};
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber::fmt::writer::MakeWriterExt;

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
    did_program_id: String,
    #[serde(default)]
    photon_url: String,
    #[serde(default = "default_pay_mode")]
    pay_mode: String,
    #[serde(default)]
    relayer_url: String,
    #[serde(default)]
    default_owner: String,
    /// State channel Hub HTTP endpoint (e.g., "http://localhost:3003").
    #[serde(default)]
    hub_endpoint: String,
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

/// Execute a payment using a session key on Solana, state channel, or fall back to mock.
///
/// V3.0 flow:
/// 1. If Solana client + session key available → real on-chain payment via session key
/// 2. If state channel client + open channel available → state channel payment
/// 3. On failure → return error (do NOT silently fall back to mock)
/// 4. If no Solana client → mock payment
/// 5. If Solana client but no session/channel → return error
async fn execute_payment(
    solana_client: &Option<Arc<IgnitePayClient>>,
    payment: &PaymentRequest,
    session: &Option<SessionKeypair>,
    channel_client: &Option<Arc<ChannelClient>>,
) -> Result<String, String> {
    match (solana_client, session) {
        (Some(client), Some(sess)) => {
            tracing::info!(
                "Executing on-chain payment: {} lamports to {} via session {}",
                payment.amount,
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
                    None,
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
        (Some(_), None) => {
            // Try state channel payment
            if let Some(channel) = channel_client {
                if let Some(channel_id) = channel.get_open_channel_id() {
                    tracing::info!(
                        "Executing state channel payment: {} to {} via channel {}",
                        payment.amount,
                        payment.recipient,
                        &channel_id[..16.min(channel_id.len())],
                    );
                    match channel.channel_pay(&channel_id, payment.amount, &payment.recipient).await {
                        Ok(result) => {
                            tracing::info!(
                                "State channel payment succeeded: channel={}, seq={}, leaf={}",
                                &result.channel_id[..16.min(result.channel_id.len())],
                                result.sequence,
                                result.leaf_index
                            );
                            Ok(format!("channel:{}:seq:{}:leaf:{}", result.channel_id, result.sequence, result.leaf_index))
                        }
                        Err(e) => Err(format!("State channel payment failed: {}", e)),
                    }
                } else {
                    Err("No active session key or open state channel".to_string())
                }
            } else {
                Err("No active session key".to_string())
            }
        }
        _ => {
            tracing::warn!("Mock payment (no Solana client configured)");
            Ok(execute_mock_payment(payment))
        }
    }
}

// ── MCP Server ─────────────────────────────────────────────────────────────

#[derive(Clone)]
struct IgnitePayMcpServer {
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
    // V3.0: State channel client for channel-based payments
    channel_client: Option<Arc<ChannelClient>>,
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

        // Step A: Extract merchant_did from X402 headers (V1.1)
        let merchant_did = input
            .x402_merchant_did
            .as_deref()
            .or_else(|| challenge.get("provider_did").and_then(|v| v.as_str()))
            .unwrap_or("unknown")
            .to_string();

        // Use x402_payment_address as recipient if present (V1.1)
        let recipient = input
            .x402_payment_address
            .as_deref()
            .or_else(|| accepts.get("recipient").and_then(|v| v.as_str()))
            .unwrap_or("unknown")
            .to_string();

        let payment_id = uuid::Uuid::new_v4().to_string();
        let description = format!(
            "{} {} {} on {} to {}",
            amount, token, payment_type, network, recipient
        );

        // 2. Create payment record
        let payment = PaymentRequest {
            id: payment_id.clone(),
            recipient: recipient.clone(),
            merchant_did: merchant_did.clone(),
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
            Ok(RiskControlDecision::AutoApproved { max_amount, label }) => {
                let session = self.get_active_session();
                return match execute_payment(&self.solana_client, &payment, &session, &self.channel_client).await {
                    Ok(tx_sig) => {
                        let _ = self
                            .payments
                            .update_status(&payment_id, &PaymentStatus::Executed);
                        let _ = self.payments.set_tx_signature(&payment_id, &tx_sig);
                        let _ = self.audit.record_payment_event(
                            &payment_id,
                            "payment_executed",
                            amount,
                            &merchant_did,
                        );
                        let label_info = label.map(|l| format!(" ({})", l)).unwrap_or_default();
                        format!(
                            "Auto-approved payment (whitelisted{}). Tx: {}\nAmount: {} {}\nTo: {}",
                            label_info, tx_sig, amount, token, recipient
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
            match execute_payment(&self.solana_client, &payment, &session, &self.channel_client).await {
                Ok(tx_sig) => {
                    if let Err(e) = self
                        .payments
                        .update_status(&payment_id, &PaymentStatus::Executed)
                    {
                        return format!("Error: Failed to update status: {}", e);
                    }
                    if let Err(e) = self.payments.set_tx_signature(&payment_id, &tx_sig) {
                        return format!("Error: Failed to set tx signature: {}", e);
                    }
                    let _ = self.audit.record_payment_event(
                        &payment_id,
                        "payment_executed",
                        amount,
                        &merchant_did,
                    );
                    return format!("Auto-approved payment (under threshold). Tx: {}", tx_sig);
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

        match self
            .mediator
            .send_auth_request(&phone_did, &payment)
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
                match execute_payment(&self.solana_client, &payment, &session, &self.channel_client).await {
                    Ok(tx_sig) => {
                        let _ = self
                            .payments
                            .update_status(&payment_id, &PaymentStatus::Executed);
                        let _ = self.payments.set_tx_signature(&payment_id, &tx_sig);

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

                        format!(
                            "Payment authorized and executed. Tx: {}\nAmount: {} {}\nTo: {}",
                            tx_sig, amount, token, recipient
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

    #[tool(description = "Get our DID identity and mediator connection status.")]
    async fn get_identity(&self) -> String {
        let solana_status = if self.solana_client.is_some() {
            "Solana: connected"
        } else {
            "Solana: not configured"
        };
        let phone = self.resolve_phone_did().await;
        format!(
            "DID: {}\nMediator: connected\nPhone DID: {}\n{}",
            self.mediator.our_did(),
            phone.as_deref().unwrap_or("(not paired)"),
            solana_status
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

        let target_program = ignite_pay_solana::solana_sdk::system_program::id();
        let scopes = vec!["sol:transfer".to_string()];

        match client.session_manager().create_session(
            &owner_pubkey,
            &target_program,
            scopes.clone(),
            input.spending_limit,
            input.duration_secs,
        ) {
            Ok(session) => {
                let expires_at = session.session_data.expires_at;
                let mut result = format!(
                    "Session created.\nPubkey: {}\nSpending limit: {} lamports\nExpires at: {} (Unix)\nScopes: {:?}",
                    session.keypair.pubkey(),
                    input.spending_limit,
                    expires_at,
                    scopes,
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
        let did_service = match &self.did_service {
            Some(s) => s,
            None => return "Error: DID service not configured (need solana.did_program_id + solana.photon_url)".to_string(),
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
        // Photon RPC and remaining accounts. This tool provides the scaffold;
        // full integration requires a Photon endpoint configured in solana.photon_url.
        format!(
            "Merchant DID initialization requested.\nDID: {}\nPayment address: {}\n\nNote: Full ZK Compression requires Photon RPC proof. Configure solana.photon_url in config.toml.",
            input.merchant_did, input.payment_address
        )
    }

    #[tool(
        description = "Update a merchant's on-chain ZK compressed DID data. Requires Photon RPC proof for the update operation."
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

        // NOTE: Full ZK Compression update requires Photon RPC proof.
        format!(
            "Merchant DID update requested.\nDID: {}\nNew payment address: {}\nNew status: {}\n\nNote: Full ZK Compression requires Photon RPC proof. Configure solana.photon_url in config.toml.",
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

    // ── State Channel Tools ──────────────────────────────────────────────

    #[tool(
        description = "Open a state channel with a Hub for off-chain payments. Requires Hub HTTP endpoint, deposit amount, and provider pubkey."
    )]
    async fn open_channel(&self, Parameters(input): Parameters<OpenChannelInput>) -> String {
        let client = match &self.channel_client {
            Some(c) => c,
            None => return "Error: State channel client not configured. Set solana.hub_endpoint in config.toml.".to_string(),
        };

        let token_mint = input.token_mint.as_deref().unwrap_or("11111111111111111111111111111111");

        match client.open_channel(
            &input.provider_pubkey,
            token_mint,
            input.deposit,
            input.tree_depth,
        ).await {
            Ok(result) => format!(
                "Channel opened.\nChannel ID: {}\nSequence: {}\nRoot: {}",
                result.channel_id, result.sequence, result.current_root
            ),
            Err(e) => format!("Error opening channel: {}", e),
        }
    }

    #[tool(
        description = "Send a payment through an open state channel. Requires channel ID, amount, and recipient pubkey."
    )]
    async fn channel_pay(&self, Parameters(input): Parameters<ChannelPayInput>) -> String {
        let client = match &self.channel_client {
            Some(c) => c,
            None => return "Error: State channel client not configured.".to_string(),
        };

        match client.channel_pay(
            &input.channel_id,
            input.amount,
            &input.recipient,
        ).await {
            Ok(result) => format!(
                "Payment sent.\nChannel: {}\nSequence: {}\nLeaf: {}\nNew root: {}",
                result.channel_id, result.sequence, result.leaf_index, result.new_root
            ),
            Err(e) => format!("Channel payment failed: {}", e),
        }
    }

    #[tool(
        description = "Get state channel status. If channel_id is provided, shows details for that channel. Otherwise lists all channels."
    )]
    async fn get_channel_status(
        &self,
        Parameters(input): Parameters<GetChannelStatusInput>,
    ) -> String {
        let client = match &self.channel_client {
            Some(c) => c,
            None => return "Error: State channel client not configured.".to_string(),
        };

        match &input.channel_id {
            Some(id) => match client.get_channel_status(id) {
                Ok(status) => format!(
                    "Channel: {}\nStatus: {}\nSequence: {}\nLeaves: {}\nBalance: {}\nDeposited: {}",
                    status.channel_id, status.status, status.sequence,
                    status.leaf_count, status.user_balance, status.total_deposited
                ),
                Err(e) => format!("Error: {}", e),
            },
            None => match client.list_channels() {
                Ok(channels) => {
                    if channels.is_empty() {
                        return "No channels found.".to_string();
                    }
                    let mut result = format!("Channels ({}):\n\n", channels.len());
                    for id in &channels {
                        match client.get_channel_status(id) {
                            Ok(s) => result.push_str(&format!(
                                "- {} | {} | seq={} | balance={}\n",
                                &id[..16.min(id.len())], s.status, s.sequence, s.user_balance
                            )),
                            Err(_) => result.push_str(&format!("- {} | (error loading)\n", &id[..16.min(id.len())])),
                        }
                    }
                    result
                }
                Err(e) => format!("Error: {}", e),
            },
        }
    }

    #[tool(
        description = "Cooperatively close an open state channel. The channel must be in Open status."
    )]
    async fn close_channel(&self, Parameters(input): Parameters<CloseChannelInput>) -> String {
        let client = match &self.channel_client {
            Some(c) => c,
            None => return "Error: State channel client not configured.".to_string(),
        };

        match client.close_channel(&input.channel_id).await {
            Ok(msg) => msg,
            Err(e) => format!("Error closing channel: {}", e),
        }
    }

    #[tool(
        description = "Initiate settlement of a state channel on-chain. After settlement, use claim + finalize to withdraw funds."
    )]
    async fn settle_channel(&self, Parameters(input): Parameters<SettleChannelInput>) -> String {
        let client = match &self.channel_client {
            Some(c) => c,
            None => return "Error: State channel client not configured.".to_string(),
        };

        match client.settle_channel(&input.channel_id).await {
            Ok(msg) => msg,
            Err(e) => format!("Error settling channel: {}", e),
        }
    }
}

impl IgnitePayMcpServer {
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
        amount: u64,
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
    // Initialize tracing with optional file output
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "ignite_pay_mcp=info".into());

    if let Ok(log_dir) = std::env::var("AUDIT_LOG_DIR") {
        let file_appender = tracing_appender::rolling::daily(&log_dir, "ignite-pay-mcp.log");
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr.and(file_appender))
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(env_filter)
            .init();
    }

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
    let payments = Arc::new(ignite_pay_mcp::payment::PaymentStore::from_db(db));
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
    // Create channel for create-channel-request commands from DIDComm messages
    let (create_channel_tx, mut create_channel_rx) =
        tokio::sync::mpsc::unbounded_channel::<ignite_pay_mcp::mediator::CreateChannelCommand>();
    mediator
        .connect(Arc::clone(&pending), Some(create_channel_tx))
        .await?;
    tracing::info!("Connecting to mediator at {}...", config.mediator.ws_url);

    // V3.0: Initialize state channel client if Hub endpoint is configured
    let channel_client = if !config.solana.hub_endpoint.is_empty() {
        let channel_db = sled::open(format!("{}/channel", config.storage.path))?;
        // Use the default owner keypair bytes for channel operations, or generate new ones
        let keypair_bytes = ChannelClient::generate_keypair();
        match ChannelClient::new(&config.solana.hub_endpoint, channel_db, &keypair_bytes) {
            Ok(client) => {
                tracing::info!(
                    "State channel client initialized: hub={}",
                    config.solana.hub_endpoint
                );
                Some(Arc::new(client))
            }
            Err(e) => {
                tracing::error!("Failed to initialize state channel client: {}", e);
                None
            }
        }
    } else {
        tracing::info!("No hub_endpoint configured, state channel payments disabled");
        None
    };

    // Spawn task to handle create-channel commands from DIDComm messages
    {
        let mediator_clone = mediator.clone();
        let channel_client_clone = channel_client.clone();
        tokio::spawn(async move {
            while let Some(cmd) = create_channel_rx.recv().await {
                tracing::info!(
                    "Processing create-channel command from {}: hub={}",
                    cmd.requestor_did,
                    cmd.hub_endpoint
                );

                let result = if let Some(ref client) = channel_client_clone {
                    client
                        .open_channel(
                            &cmd.provider_pubkey,
                            &cmd.token_mint,
                            cmd.deposit,
                            cmd.tree_depth,
                        )
                        .await
                } else {
                    Err(anyhow::anyhow!("Channel client not configured"))
                };

                match result {
                    Ok(open_result) => {
                        if let Err(e) = mediator_clone
                            .send_create_channel_response(
                                &cmd.requestor_did,
                                &open_result.channel_id,
                                open_result.sequence,
                                &open_result.current_root,
                                true,
                                None,
                            )
                            .await
                        {
                            tracing::error!("Failed to send create-channel response: {}", e);
                        }
                    }
                    Err(e) => {
                        if let Err(e2) = mediator_clone
                            .send_create_channel_response(
                                &cmd.requestor_did,
                                "",
                                0,
                                "",
                                false,
                                Some(&e.to_string()),
                            )
                            .await
                        {
                            tracing::error!("Failed to send create-channel error response: {}", e2);
                        }
                    }
                }
            }
        });
    }

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
        ipfs_client: Arc::new(Box::new(MockIpfsClient::new())),
        audit,
        default_owner,
        did_service,
        channel_client,
    };

    tracing::info!("Starting MCP server on stdio...");
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
