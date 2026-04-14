mod mediator;
mod payment;
mod tools;

use crate::mediator::MediatorConnection;
use crate::payment::{
    execute_mock_payment, AuthResponse, PaymentRequest, PaymentStatus, PendingAuthStore,
};
use crate::tools::{
    AuthorizationCheckInput, CloseSessionInput, CreateSessionInput, PaymentHistoryInput,
    SessionStatusInput, X402ChallengeInput,
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
use ignite_pay_solana::types::{PayMode, SessionTokenData};

use rmcp::{
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::ServerCapabilities,
    tool, tool_handler, tool_router,
    transport::stdio,
    ServerHandler, ServiceExt,
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
    let config_path =
        std::env::var("IGNITE_PAY_CONFIG").unwrap_or_else(|_| "config.toml".to_string());
    let content = std::fs::read_to_string(&config_path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

// ── Payment Execution ──────────────────────────────────────────────────────

/// Execute a payment using a session key on Solana, or fall back to mock.
///
/// V2.0 flow:
/// 1. If Solana client + session key available → real on-chain payment via session key
/// 2. On failure → fall back to mock payment
/// 3. If no Solana client or session → mock payment
async fn execute_payment(
    solana_client: &Option<Arc<IgnitePayClient>>,
    payment: &PaymentRequest,
    session: &Option<SessionKeypair>,
) -> String {
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
                )
                .await
            {
                Ok(result) => {
                    tracing::info!(
                        "On-chain payment succeeded: sig={}, slot={}",
                        result.signature,
                        result.slot
                    );
                    result.signature
                }
                Err(e) => {
                    tracing::warn!("On-chain payment failed (falling back to mock): {}", e);
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
    // V1.1: IPFS client for VC resolution and list sync
    ipfs_client: Arc<Box<dyn IpfsClient>>,
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

        // 3.7 V1.1: Merkle proof verification from x402-merkle-context
        if let Some(merkle_ctx_str) = &input.x402_merkle_context {
            if let Some(bridge) = &self.solana_bridge {
                match serde_json::from_str::<serde_json::Value>(merkle_ctx_str) {
                    Ok(ctx) => {
                        if let Some(leaf_index) = ctx.get("leaf_index").and_then(|v| v.as_u64()) {
                            // Extract proof_nodes as array of base64 strings
                            let proof_nodes_raw = ctx.get("proof_nodes").and_then(|v| v.as_array());
                            let proof_nodes: Vec<Vec<u8>> = proof_nodes_raw
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| {
                                            v.as_str().and_then(|s| {
                                                base64::engine::general_purpose::STANDARD_NO_PAD
                                                    .decode(s)
                                                    .ok()
                                            })
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();

                            match bridge
                                .verify_merchant_with_proof(
                                    &merchant_did,
                                    leaf_index as u32,
                                    &proof_nodes,
                                )
                                .await
                            {
                                Ok(true) => {
                                    tracing::info!(
                                        "Merkle proof verified for merchant {}",
                                        merchant_did
                                    );
                                }
                                Ok(false) => {
                                    let _ = self
                                        .payments
                                        .update_status(&payment_id, &PaymentStatus::Rejected);
                                    return format!(
                                        "Payment rejected: Merkle proof verification failed for {}",
                                        merchant_did
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Merkle proof verification error (continuing): {}",
                                        e
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse x402-merkle-context: {}", e);
                    }
                }
            }
        }

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
                let tx_sig = execute_payment(&self.solana_client, &payment, &session).await;
                let _ = self
                    .payments
                    .update_status(&payment_id, &PaymentStatus::Executed);
                let _ = self.payments.set_tx_signature(&payment_id, &tx_sig);
                let label_info = label.map(|l| format!(" ({})", l)).unwrap_or_default();
                return format!(
                    "Auto-approved payment (whitelisted{}). Tx: {}\nAmount: {} {}\nTo: {}",
                    label_info, tx_sig, amount, token, recipient
                );
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
            return format!("Auto-approved payment (under threshold). Tx: {}", tx_sig);
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
                let tx_sig = execute_payment(&self.solana_client, &payment, &session).await;
                let _ = self
                    .payments
                    .update_status(&payment_id, &PaymentStatus::Executed);
                let _ = self.payments.set_tx_signature(&payment_id, &tx_sig);

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

    #[tool(
        description = "Create a local session key for testing/auto-approved payments. Returns the session key pubkey, spending limit, and expiry."
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

        let target_program = solana_sdk::system_program::id();
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
                format!(
                    "Session created.\nPubkey: {}\nSpending limit: {} lamports\nExpires at: {} (Unix)\nScopes: {:?}",
                    session.keypair.pubkey(),
                    input.spending_limit,
                    expires_at,
                    scopes,
                )
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
}

impl IgnitePayMcpServer {
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

        // Upload updated lists to IPFS and notify phone
        match self
            .list_store
            .upload_to_ipfs(self.ipfs_client.as_ref())
            .await
        {
            Ok(new_cid) => {
                tracing::info!("Lists uploaded to IPFS: {}", new_cid);
                // Send list-sync-notification to phone
                if let Err(e) = self
                    .mediator
                    .send_list_sync_notification(
                        &self.phone_did,
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
            Err(e) => {
                tracing::warn!("Failed to upload lists to IPFS: {}", e);
            }
        }
    }

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
            owner: Pubkey::default(), // MCP doesn't know the real owner; will use session key as payer
            ephemeral_signer: keypair.pubkey(),
            target_program: solana_sdk::system_program::id(),
            expires_at,
            spending_limit,
            current_spent: 0,
            scopes,
        };

        // Store in sled for reuse
        if let Some(client) = &self.solana_client {
            let key = format!("remote_session:{}", keypair.pubkey());
            let mut value = borsh::to_vec(&session_data).unwrap_or_default();
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
    let mediator = Arc::new(MediatorConnection::new(&config.mediator.ws_url, &db)?);

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
        ipfs_client: Arc::new(Box::new(MockIpfsClient::new())),
    };

    tracing::info!("Starting MCP server on stdio...");
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
