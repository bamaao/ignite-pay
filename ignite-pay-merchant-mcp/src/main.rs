use ignite_pay_merchant_mcp::audit::AuditLogStore;
use ignite_pay_merchant_mcp::config;
use ignite_pay_merchant_mcp::mediator::MerchantMediator;
use ignite_pay_merchant_mcp::payment::{PaymentOrder, PaymentOrderStore};
use ignite_pay_merchant_mcp::qr::{self, PaymentQrData};
use ignite_pay_merchant_mcp::settlement_store::SettlementStore;
use ignite_pay_merchant_mcp::tools::{
    CheckPaymentInput, CreateOrderInput, GeneratePaymentQrInput, GetPaymentHistoryInput,
    ListProductsInput,
    MbForceReleaseInput, MbGetChannelInput, MbGetSettlementInput,
    MbOptimisticSettleInput, MbReceiveVoucherInput, MbReleaseSettlementInput,
    MbSettleBatchInput, RegisterMerchantInput, VerifyMerchantDidInput,
    VerifyPaymentInput,
};
use ignite_pay_merchant_mcp::voucher_store::{CollectedVoucher, MerchantVoucherStore};

use ignite_pay_mb_sdk::{merkle, pda, signing, transaction};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::keypair::Keypair as MbKeypair;
use solana_sdk::signer::Signer as MbSigner;

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
use tracing_subscriber::fmt::writer::MakeWriterExt;

// ── Account Deserialization ─────────────────────────────────────────────────

struct ChannelAccount {
    buyer: Pubkey,
    merchant: Pubkey,
    spending_cap: u64,
    settled_amount: u64,
    nonce: u64,
    challenge_period: i64,
    dispute_period: i64,
    _bump: u8,
}

struct EscrowAccount {
    channel: Pubkey,
    merchant: Pubkey,
    amount: u64,
    merkle_root: [u8; 32],
    nonce: u64,
    created_at: i64,
    claimed: bool,
    disputed: bool,
    optimistic: bool,
    _bump: u8,
}

fn deserialize_channel(data: &[u8]) -> anyhow::Result<ChannelAccount> {
    if data.len() < 8 + 32 + 32 + 8 + 8 + 8 + 8 + 8 + 1 {
        return Err(anyhow::anyhow!("Channel account data too short"));
    }
    let d = &data[8..];
    Ok(ChannelAccount {
        buyer: Pubkey::try_from(&d[0..32])?,
        merchant: Pubkey::try_from(&d[32..64])?,
        spending_cap: u64::from_le_bytes(d[64..72].try_into()?),
        settled_amount: u64::from_le_bytes(d[72..80].try_into()?),
        nonce: u64::from_le_bytes(d[80..88].try_into()?),
        challenge_period: i64::from_le_bytes(d[88..96].try_into()?),
        dispute_period: i64::from_le_bytes(d[96..104].try_into()?),
        _bump: d[104],
    })
}

fn deserialize_escrow(data: &[u8]) -> anyhow::Result<EscrowAccount> {
    // Anchor: 8-byte discriminator + channel(32) + merchant(32) + amount(8) +
    // merkle_root(32) + nonce(8) + created_at(8) + claimed(1) + disputed(1) +
    // optimistic(1) + bump(1)
    if data.len() < 8 + 32 + 32 + 8 + 32 + 8 + 8 + 1 + 1 + 1 + 1 {
        return Err(anyhow::anyhow!("Escrow account data too short"));
    }
    let d = &data[8..];
    let mut merkle_root = [0u8; 32];
    merkle_root.copy_from_slice(&d[72..104]);
    Ok(EscrowAccount {
        channel: Pubkey::try_from(&d[0..32])?,
        merchant: Pubkey::try_from(&d[32..64])?,
        amount: u64::from_le_bytes(d[64..72].try_into()?),
        merkle_root,
        nonce: u64::from_le_bytes(d[104..112].try_into()?),
        created_at: i64::from_le_bytes(d[112..120].try_into()?),
        claimed: d[120] != 0,
        disputed: d[121] != 0,
        optimistic: d[122] != 0,
        _bump: d[123],
    })
}

// ── MCP Server ─────────────────────────────────────────────────────────────

#[derive(Clone)]
struct MerchantMcpServer {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    mediator: Arc<MerchantMediator>,
    orders: Arc<PaymentOrderStore>,
    audit: Arc<AuditLogStore>,
    // MagicBlock payment channels
    mb_rpc: Arc<RpcClient>,
    mb_program_id: Pubkey,
    mb_merchant_keypair: Arc<MbKeypair>,
    mb_voucher_store: Arc<MerchantVoucherStore>,
    mb_settlement_store: Arc<SettlementStore>,
    // QR payment config
    merchant_wallet: String,
    default_accept_tokens: Vec<String>,
    // DID Registry
    did_registry_url: String,
    // Agent x402 payment
    products: Vec<config::ProductConfig>,
    solana_rpc_url: String,
}

#[tool_router]
impl MerchantMcpServer {
    #[tool(description = "Generate a payment QR code for receiving payments. Returns QR text and ASCII representation.")]
    async fn generate_payment_qr(
        &self,
        Parameters(input): Parameters<GeneratePaymentQrInput>,
    ) -> String {
        let order_id = input
            .order_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let accept_tokens = input.accept_tokens
            .unwrap_or_else(|| self.default_accept_tokens.clone());

        let qr_data = PaymentQrData {
            qr_type: "ignite-pay-request".to_string(),
            version: 1,
            merchant_did: self.mediator.our_did().to_string(),
            amount: input.amount,
            description: input.description.clone(),
            order_id: order_id.clone(),
            hub_endpoint: self.mb_merchant_keypair.pubkey().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            merchant_mb_pubkey: self.mb_merchant_keypair.pubkey().to_string(),
            merchant_wallet: self.merchant_wallet.clone(),
            accept_tokens,
        };

        let order = PaymentOrder {
            order_id: order_id.clone(),
            merchant_did: self.mediator.our_did().to_string(),
            amount: input.amount,
            description: input.description.clone(),
            hub_endpoint: self.mb_merchant_keypair.pubkey().to_string(),
            status: ignite_pay_merchant_mcp::payment::OrderStatus::Pending,
            created_at: chrono::Utc::now(),
            confirmed_at: None,
            channel_id: None,
            leaf_index: None,
            sequence: None,
        };
        if let Err(e) = self.orders.save_order(&order) {
            return format!("Error saving order: {}", e);
        }

        let _ = self.audit.append("qr_generated", Some(&order_id), Some(input.amount), "Payment QR generated");

        let qr_text = qr::generate_payment_qr_text(&qr_data);
        let ascii = match qr::generate_qr_ascii(&qr_data) {
            Ok(a) => a,
            Err(e) => format!("(QR render error: {})", e),
        };

        format!(
            "Payment QR generated.\nOrder: {}\nAmount: {}\nDescription: {}\n\nQR Text: {}\n\nASCII:\n{}",
            order_id, input.amount, input.description, qr_text, ascii
        )
    }

    #[tool(description = "Check the status of a payment order by order_id.")]
    async fn check_payment(&self, Parameters(input): Parameters<CheckPaymentInput>) -> String {
        match self.orders.get_order(&input.order_id) {
            Ok(Some(order)) => {
                let confirmed = order
                    .confirmed_at
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_else(|| "N/A".to_string());
                format!(
                    "Order: {}\nMerchant: {}\nAmount: {}\nDescription: {}\nStatus: {}\nCreated: {}\nConfirmed: {}\nChannel: {}\nLeaf: {}",
                    order.order_id,
                    order.merchant_did,
                    order.amount,
                    order.description,
                    order.status,
                    order.created_at.to_rfc3339(),
                    confirmed,
                    order.channel_id.as_deref().unwrap_or("N/A"),
                    order.leaf_index.map(|i| i.to_string()).as_deref().unwrap_or("N/A"),
                )
            }
            Ok(None) => format!("Order {} not found.", input.order_id),
            Err(e) => format!("Error: {}", e),
        }
    }

    #[tool(description = "Get recent payment history.")]
    async fn get_payment_history(
        &self,
        Parameters(input): Parameters<GetPaymentHistoryInput>,
    ) -> String {
        match self.orders.list_orders(input.limit) {
            Ok(orders) => {
                if orders.is_empty() {
                    return "No payment orders found.".to_string();
                }
                let mut result = format!("Payment History ({} orders):\n\n", orders.len());
                for order in &orders {
                    result.push_str(&format!(
                        "- {} | {} | {} | {} | {}\n",
                        &order.order_id[..8.min(order.order_id.len())],
                        order.status,
                        order.amount,
                        order.description.chars().take(20).collect::<String>(),
                        order.created_at.format("%Y-%m-%d %H:%M"),
                    ));
                }
                result
            }
            Err(e) => format!("Error: {}", e),
        }
    }

    #[tool(description = "Get merchant identity: DID, MagicBlock pubkey, and mediator connection info.")]
    async fn get_identity(&self, Parameters(_): Parameters<()>) -> String {
        let did = self.mediator.our_did();
        format!(
            "Merchant DID: {}\nMB Merchant: {}\nMB Program: {}",
            did,
            self.mb_merchant_keypair.pubkey(),
            self.mb_program_id,
        )
    }

    // ── MagicBlock Payment Channel Tools ──────────────────────────────────

    #[tool(description = "Get the on-chain state of a payment channel with a buyer.")]
    async fn mb_get_channel(&self, Parameters(input): Parameters<MbGetChannelInput>) -> String {
        let buyer = match input.buyer_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid buyer pubkey: {}", e),
        };

        let token_mint = Pubkey::default();
        let (channel_pda, _) = pda::derive_channel_pda(
            &self.mb_program_id,
            &buyer,
            &self.mb_merchant_keypair.pubkey(),
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
        description = "Receive a signed voucher from a buyer. Verifies the buyer's signature and stores the voucher for batch settlement."
    )]
    async fn mb_receive_voucher(&self, Parameters(input): Parameters<MbReceiveVoucherInput>) -> String {
        let buyer = match input.buyer_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid buyer pubkey: {}", e),
        };

        let buyer_sig = match bs58::decode(&input.buyer_sig).into_vec() {
            Ok(v) if v.len() == 64 => {
                let mut arr = [0u8; 64];
                arr.copy_from_slice(&v);
                arr
            }
            _ => return "Error: Invalid buyer_sig (must be 64 bytes, base58)".to_string(),
        };

        let token_mint = Pubkey::default();
        let (channel_pda, _) = pda::derive_channel_pda(
            &self.mb_program_id,
            &buyer,
            &self.mb_merchant_keypair.pubkey(),
            &token_mint,
        );
        let channel_id = channel_pda.to_bytes();

        // Verify the buyer's signature
        let msg_hash = {
            use sha2::{Sha256, Digest};
            let mut hasher = Sha256::new();
            hasher.update(&channel_id);
            hasher.update(&input.seq.to_be_bytes());
            hasher.update(&input.amount.to_be_bytes());
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&hasher.finalize());
            hash
        };

        if !signing::verify_signature(&buyer.to_bytes(), &msg_hash, &buyer_sig) {
            return "Error: Buyer signature verification failed".to_string();
        }

        let voucher = CollectedVoucher {
            channel_id,
            buyer: buyer.to_bytes(),
            seq: input.seq,
            amount: input.amount,
            buyer_sig,
        };

        if let Err(e) = self.mb_voucher_store.store_voucher(&voucher) {
            return format!("Error storing voucher: {}", e);
        }

        // Optionally confirm the order if order_id was provided
        if let Some(ref order_id) = input.order_id {
            if let Err(e) = self.orders.confirm_order(order_id, &channel_pda.to_string(), 0, input.seq) {
                tracing::warn!("Failed to confirm order {}: {}", order_id, e);
            }
        }

        format!(
            "Voucher received and verified.\nChannel: {}\nSeq: {}\nAmount: {}",
            channel_pda, input.seq, input.amount
        )
    }

    #[tool(
        description = "Settle a batch of vouchers on-chain. Builds a merkle tree from collected vouchers, signs the merchant side, and submits the settlement transaction."
    )]
    async fn mb_settle_batch(&self, Parameters(input): Parameters<MbSettleBatchInput>) -> String {
        let buyer = match input.buyer_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid buyer pubkey: {}", e),
        };

        let buyer_batch_sig = match bs58::decode(&input.buyer_batch_sig).into_vec() {
            Ok(v) if v.len() == 64 => {
                let mut arr = [0u8; 64];
                arr.copy_from_slice(&v);
                arr
            }
            _ => return "Error: Invalid buyer_batch_sig (must be 64 bytes, base58)".to_string(),
        };

        let token_mint = Pubkey::default();
        let (channel_pda, _) = pda::derive_channel_pda(
            &self.mb_program_id,
            &buyer,
            &self.mb_merchant_keypair.pubkey(),
            &token_mint,
        );
        let channel_id = channel_pda.to_bytes();

        // Get collected vouchers
        let collected = match self.mb_voucher_store.get_vouchers_for_channel(&channel_id) {
            Ok(v) => v,
            Err(e) => return format!("Error loading vouchers: {}", e),
        };

        if collected.is_empty() {
            return "Error: No vouchers found for this channel".to_string();
        }

        // Build merkle tree
        let vouchers: Vec<merkle::Voucher> = collected.iter().map(|v| merkle::Voucher {
            channel_id: v.channel_id,
            seq: v.seq,
            amount: v.amount,
            buyer_pubkey: v.buyer,
            buyer_sig: v.buyer_sig,
        }).collect();

        let tree = merkle::build_sum_merkle_tree(&vouchers);
        let merkle_root = tree.root_hash();
        let total_amount = tree.root_sum();

        // Get channel to determine nonce
        let channel_data = match self.mb_rpc.get_account(&channel_pda) {
            Ok(a) => a,
            Err(e) => return format!("Error fetching channel: {}", e),
        };
        let channel = match deserialize_channel(&channel_data.data) {
            Ok(c) => c,
            Err(e) => return format!("Error deserializing channel: {}", e),
        };
        let nonce = channel.nonce;

        // Derive escrow PDA
        let (escrow_pda, _) = pda::derive_settlement_pda(&self.mb_program_id, &channel_pda, nonce);

        // Sign merchant side
        let kp_bytes = self.mb_merchant_keypair.to_bytes();
        let merchant_batch_sig = signing::sign_settlement(
            &signing::build_settlement_message(&merkle_root, total_amount, &channel_id, nonce),
            &kp_bytes,
        );

        let recent_blockhash = match self.mb_rpc.get_latest_blockhash() {
            Ok(bh) => bh,
            Err(e) => return format!("Error getting blockhash: {}", e),
        };

        let token_mint = Pubkey::default();
        let tx = match transaction::build_settle_batch_tx(
            &*self.mb_merchant_keypair,
            &buyer,
            &channel_pda,
            &escrow_pda,
            &token_mint,
            &merkle_root,
            total_amount,
            &buyer_batch_sig,
            &merchant_batch_sig,
            nonce,
            &self.mb_program_id,
            recent_blockhash,
        ) {
            Ok(tx) => tx,
            Err(e) => return format!("Error building tx: {}", e),
        };

        match self.mb_rpc.send_and_confirm_transaction(&tx) {
            Ok(sig) => {
                // Record settlement
                let record = ignite_pay_merchant_mcp::settlement_store::SettlementRecord {
                    channel_id,
                    buyer: buyer.to_bytes(),
                    nonce,
                    amount: total_amount,
                    merkle_root,
                    tx_signature: sig.to_string(),
                };
                if let Err(e) = self.mb_settlement_store.record_settlement(&record) {
                    tracing::warn!("Failed to record settlement: {}", e);
                }
                format!(
                    "Batch settled.\nChannel: {}\nEscrow: {}\nMerkle root: {}\nTotal: {}\nNonce: {}\nSignature: {}",
                    channel_pda, escrow_pda,
                    bs58::encode(merkle_root).into_string(),
                    total_amount, nonce, sig
                )
            }
            Err(e) => format!("Error sending tx: {}", e),
        }
    }

    #[tool(
        description = "Optimistically settle a channel without buyer cooperation. Funds go to escrow and must wait for challenge period."
    )]
    async fn mb_optimistic_settle(&self, Parameters(input): Parameters<MbOptimisticSettleInput>) -> String {
        let buyer = match input.buyer_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid buyer pubkey: {}", e),
        };

        let token_mint = Pubkey::default();
        let (channel_pda, _) = pda::derive_channel_pda(
            &self.mb_program_id,
            &buyer,
            &self.mb_merchant_keypair.pubkey(),
            &token_mint,
        );
        let channel_id = channel_pda.to_bytes();

        // Get collected vouchers
        let collected = match self.mb_voucher_store.get_vouchers_for_channel(&channel_id) {
            Ok(v) => v,
            Err(e) => return format!("Error loading vouchers: {}", e),
        };

        if collected.is_empty() {
            return "Error: No vouchers found for this channel".to_string();
        }

        let vouchers: Vec<merkle::Voucher> = collected.iter().map(|v| merkle::Voucher {
            channel_id: v.channel_id,
            seq: v.seq,
            amount: v.amount,
            buyer_pubkey: v.buyer,
            buyer_sig: v.buyer_sig,
        }).collect();

        let tree = merkle::build_sum_merkle_tree(&vouchers);
        let merkle_root = tree.root_hash();
        let total_amount = tree.root_sum();

        let channel_data = match self.mb_rpc.get_account(&channel_pda) {
            Ok(a) => a,
            Err(e) => return format!("Error fetching channel: {}", e),
        };
        let channel = match deserialize_channel(&channel_data.data) {
            Ok(c) => c,
            Err(e) => return format!("Error deserializing channel: {}", e),
        };
        let nonce = channel.nonce;

        let (escrow_pda, _) = pda::derive_settlement_pda(&self.mb_program_id, &channel_pda, nonce);

        let kp_bytes = self.mb_merchant_keypair.to_bytes();
        let merchant_batch_sig = signing::sign_settlement(
            &signing::build_settlement_message(&merkle_root, total_amount, &channel_id, nonce),
            &kp_bytes,
        );

        let recent_blockhash = match self.mb_rpc.get_latest_blockhash() {
            Ok(bh) => bh,
            Err(e) => return format!("Error getting blockhash: {}", e),
        };

        let token_mint = Pubkey::default();
        let tx = match transaction::build_optimistic_settle_tx(
            &*self.mb_merchant_keypair,
            &buyer,
            &channel_pda,
            &escrow_pda,
            &token_mint,
            &merkle_root,
            total_amount,
            &merchant_batch_sig,
            nonce,
            &self.mb_program_id,
            recent_blockhash,
        ) {
            Ok(tx) => tx,
            Err(e) => return format!("Error building tx: {}", e),
        };

        match self.mb_rpc.send_and_confirm_transaction(&tx) {
            Ok(sig) => format!(
                "Optimistic settlement submitted.\nChannel: {}\nEscrow: {}\nTotal: {}\nNonce: {}\nSignature: {}",
                channel_pda, escrow_pda, total_amount, nonce, sig
            ),
            Err(e) => format!("Error sending tx: {}", e),
        }
    }

    #[tool(description = "Get the on-chain state of a settlement escrow.")]
    async fn mb_get_settlement(&self, Parameters(input): Parameters<MbGetSettlementInput>) -> String {
        let buyer = match input.buyer_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid buyer pubkey: {}", e),
        };

        let token_mint = Pubkey::default();
        let (channel_pda, _) = pda::derive_channel_pda(
            &self.mb_program_id,
            &buyer,
            &self.mb_merchant_keypair.pubkey(),
            &token_mint,
        );
        let (escrow_pda, _) = pda::derive_settlement_pda(
            &self.mb_program_id,
            &channel_pda,
            input.batch_nonce,
        );

        let account = match self.mb_rpc.get_account(&escrow_pda) {
            Ok(a) => a,
            Err(e) => return format!("Error: Escrow not found: {}", e),
        };

        match deserialize_escrow(&account.data) {
            Ok(esc) => format!(
                "Escrow: {}\nChannel: {}\nMerchant: {}\nAmount: {}\nMerkle root: {}\nNonce: {}\nCreated at: {}\nClaimed: {}\nDisputed: {}\nOptimistic: {}",
                escrow_pda, esc.channel, esc.merchant, esc.amount,
                bs58::encode(esc.merkle_root).into_string(),
                esc.nonce, esc.created_at, esc.claimed, esc.disputed, esc.optimistic
            ),
            Err(e) => format!("Error deserializing escrow: {}", e),
        }
    }

    #[tool(description = "Release a settlement after the challenge period has passed. Transfers funds from escrow to the merchant.")]
    async fn mb_release_settlement(&self, Parameters(input): Parameters<MbReleaseSettlementInput>) -> String {
        let buyer = match input.buyer_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid buyer pubkey: {}", e),
        };

        let token_mint = Pubkey::default();
        let (channel_pda, _) = pda::derive_channel_pda(
            &self.mb_program_id,
            &buyer,
            &self.mb_merchant_keypair.pubkey(),
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

        let tx = match transaction::build_release_settlement_tx(
            &*self.mb_merchant_keypair,
            &channel_pda,
            &escrow_pda,
            &self.mb_program_id,
            recent_blockhash,
        ) {
            Ok(tx) => tx,
            Err(e) => return format!("Error building tx: {}", e),
        };

        match self.mb_rpc.send_and_confirm_transaction(&tx) {
            Ok(sig) => format!("Settlement released. Signature: {}", sig),
            Err(e) => format!("Error sending tx: {}", e),
        }
    }

    #[tool(description = "Force release a settlement after dispute period has passed without buyer response.")]
    async fn mb_force_release(&self, Parameters(input): Parameters<MbForceReleaseInput>) -> String {
        let buyer = match input.buyer_pubkey.parse::<Pubkey>() {
            Ok(p) => p,
            Err(e) => return format!("Error: Invalid buyer pubkey: {}", e),
        };

        let token_mint = Pubkey::default();
        let (channel_pda, _) = pda::derive_channel_pda(
            &self.mb_program_id,
            &buyer,
            &self.mb_merchant_keypair.pubkey(),
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

        let tx = match transaction::build_force_release_tx(
            &*self.mb_merchant_keypair,
            &channel_pda,
            &escrow_pda,
            &self.mb_program_id,
            recent_blockhash,
        ) {
            Ok(tx) => tx,
            Err(e) => return format!("Error building tx: {}", e),
        };

        match self.mb_rpc.send_and_confirm_transaction(&tx) {
            Ok(sig) => format!("Force release executed. Signature: {}", sig),
            Err(e) => format!("Error sending tx: {}", e),
        }
    }

    // ── DID Registry Tools ──────────────────────────────────────────────

    #[tool(description = "Register merchant identity: obtain platform VC and register on-chain (PDA by default)")]
    async fn register_merchant(&self, Parameters(input): Parameters<RegisterMerchantInput>) -> String {
        let registry_url = &self.did_registry_url;
        if registry_url.is_empty() {
            return "Error: did_registry.url not configured. Add [did_registry] section to config.toml.".to_string();
        }

        let merchant_did = self.mediator.our_did().to_string();
        let client = reqwest::Client::new();

        // Step 1: GET /v1/auth/nonce
        let nonce_resp = match client.get(format!("{}/v1/auth/nonce", registry_url)).send().await {
            Ok(r) => r,
            Err(e) => return format!("Error fetching nonce: {}", e),
        };
        let nonce_body: serde_json::Value = match nonce_resp.json().await {
            Ok(v) => v,
            Err(e) => return format!("Error parsing nonce response: {}", e),
        };
        let nonce = match nonce_body["nonce"].as_str() {
            Some(n) => n.to_string(),
            None => return format!("Error: missing nonce in response: {:?}", nonce_body),
        };

        // Step 2: Sign "issue_vc:{did}:{name}:{nonce}"
        let vc_msg = format!("issue_vc:{}:{}:{}", merchant_did, input.merchant_name, nonce);
        let vc_sig = self.mediator.sign(vc_msg.as_bytes());

        // Step 3: POST /v1/vc/issue
        let mut vc_body = serde_json::json!({
            "merchant_did": merchant_did,
            "merchant_name": input.merchant_name,
            "nonce": nonce,
            "did_signature": vc_sig,
        });
        if let Some(ref cat) = input.category {
            vc_body["category"] = serde_json::Value::String(cat.clone());
        }

        let vc_resp = match client
            .post(format!("{}/v1/vc/issue", registry_url))
            .json(&vc_body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return format!("Error requesting VC: {}", e),
        };
        let vc_result: serde_json::Value = match vc_resp.json().await {
            Ok(v) => v,
            Err(e) => return format!("Error parsing VC response: {}", e),
        };
        let vc_hash = match vc_result["vc_hash"].as_str() {
            Some(h) => h.to_string(),
            None => return format!("Error: missing vc_hash in VC response: {:?}", vc_result),
        };

        // Step 4: GET /v1/auth/nonce (fresh nonce for register)
        let nonce_resp2 = match client.get(format!("{}/v1/auth/nonce", registry_url)).send().await {
            Ok(r) => r,
            Err(e) => return format!("Error fetching second nonce: {}", e),
        };
        let nonce_body2: serde_json::Value = match nonce_resp2.json().await {
            Ok(v) => v,
            Err(e) => return format!("Error parsing second nonce response: {}", e),
        };
        let nonce2 = match nonce_body2["nonce"].as_str() {
            Some(n) => n.to_string(),
            None => return format!("Error: missing nonce in second response: {:?}", nonce_body2),
        };

        // Step 5: Sign "register:{did}:{active_pubkey}:{vc_hash}:{nonce}"
        let active_pubkey = self.mb_merchant_keypair.pubkey().to_string();
        let reg_msg = format!("register:{}:{}:{}:{}", merchant_did, active_pubkey, vc_hash, nonce2);
        let reg_sig = self.mediator.sign(reg_msg.as_bytes());

        // Step 6: POST /v1/merchants/register
        let reg_body = serde_json::json!({
            "merchant_did": merchant_did,
            "active_pubkey": active_pubkey,
            "platform_vc_hash": vc_hash,
            "did_signature": reg_sig,
            "nonce": nonce2,
            "mode": "sponsored",
        });

        let reg_resp = match client
            .post(format!("{}/v1/merchants/register", registry_url))
            .json(&reg_body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return format!("Error registering merchant: {}", e),
        };
        let reg_result: serde_json::Value = match reg_resp.json().await {
            Ok(v) => v,
            Err(e) => return format!("Error parsing register response: {}", e),
        };

        format!(
            "Merchant registered.\nDID: {}\nVC hash: {}\nRegister response: {}",
            merchant_did,
            vc_hash,
            serde_json::to_string_pretty(&reg_result).unwrap_or_else(|_| reg_result.to_string())
        )
    }

    #[tool(description = "Check if the merchant DID is registered on-chain")]
    async fn verify_merchant_did(&self, Parameters(_input): Parameters<VerifyMerchantDidInput>) -> String {
        let registry_url = &self.did_registry_url;
        if registry_url.is_empty() {
            return "Error: did_registry.url not configured. Add [did_registry] section to config.toml.".to_string();
        }

        let merchant_did = self.mediator.our_did().to_string();
        let client = reqwest::Client::new();

        let resp = match client
            .get(format!("{}/v1/merchants/verify/{}", registry_url, merchant_did))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return format!("Error verifying DID: {}", e),
        };
        let result: serde_json::Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => return format!("Error parsing verify response: {}", e),
        };

        format!(
            "DID verification for {}:\n{}",
            merchant_did,
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())
        )
    }

    // ── Agent x402 Payment Tools ──────────────────────────────────────────

    #[tool(description = "List available products/services with pricing")]
    async fn list_products(&self, Parameters(_): Parameters<ListProductsInput>) -> String {
        if self.products.is_empty() {
            return "No products configured. Add [[products]] to config.toml.".to_string();
        }
        serde_json::to_string_pretty(&self.products).unwrap_or_else(|e| format!("Error: {}", e))
    }

    #[tool(description = "Create a payment order and return x402 challenge for Agent payment")]
    async fn create_order(&self, Parameters(input): Parameters<CreateOrderInput>) -> String {
        // 1. Resolve amount
        let (amount, description) = if let Some(ref pid) = input.product_id {
            match self.products.iter().find(|p| &p.id == pid) {
                Some(product) => (product.price, if input.description.is_some() { input.description.clone() } else { Some(product.name.clone()) }),
                None => return format!("Error: Product '{}' not found. Use list_products to see available products.", pid),
            }
        } else if let Some(amt) = input.amount {
            (amt, input.description.clone())
        } else {
            return "Error: Either product_id or amount is required.".to_string();
        };

        // 2. Generate order_id
        let order_id = uuid::Uuid::new_v4().to_string();
        let desc = description.unwrap_or_else(|| "x402 payment".to_string());

        // 3. Create and save PaymentOrder
        let order = PaymentOrder {
            order_id: order_id.clone(),
            merchant_did: self.mediator.our_did().to_string(),
            amount,
            description: desc.clone(),
            hub_endpoint: self.mb_merchant_keypair.pubkey().to_string(),
            status: ignite_pay_merchant_mcp::payment::OrderStatus::Pending,
            created_at: chrono::Utc::now(),
            confirmed_at: None,
            channel_id: None,
            leaf_index: None,
            sequence: None,
        };
        if let Err(e) = self.orders.save_order(&order) {
            return format!("Error saving order: {}", e);
        }

        let _ = self.audit.append("order_created", Some(&order_id), Some(amount), "x402 order created");

        // 4. Build x402 challenge JSON
        let merchant_did = self.mediator.our_did().to_string();
        let payment_address = self.merchant_wallet.clone();

        let challenge = serde_json::json!({
            "order_id": order_id,
            "x402_challenge": {
                "scheme": "exact",
                "network": "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
                "maxTimeoutSeconds": 300,
                "amount": amount.to_string(),
                "asset": "So11111111111111111111111111111111111111112",
                "payTo": payment_address,
                "extra": {
                    "memo": merchant_did,
                    "order_id": order_id,
                }
            },
            "x402_merchant_did": merchant_did,
            "x402_payment_address": self.merchant_wallet,
        });

        serde_json::to_string_pretty(&challenge).unwrap_or_else(|e| format!("Error: {}", e))
    }

    #[tool(description = "Verify payment proof for an order")]
    async fn verify_payment(&self, Parameters(input): Parameters<VerifyPaymentInput>) -> String {
        // 1. Lookup order
        let order = match self.orders.get_order(&input.order_id) {
            Ok(Some(o)) => o,
            Ok(None) => return format!("Error: Order '{}' not found.", input.order_id),
            Err(e) => return format!("Error loading order: {}", e),
        };

        if order.status != ignite_pay_merchant_mcp::payment::OrderStatus::Pending {
            return format!("Order '{}' status is {} (expected pending).", input.order_id, order.status);
        }

        // 2. Parse payment proof
        let proof = input.payment_proof.trim();

        if let Some(sig) = proof.strip_prefix("Tx: ") {
            // 3a. On-chain tx verification via Solana JSON-RPC
            let signature = sig.trim();
            verify_on_chain_tx(&self.solana_rpc_url, signature, &self.merchant_wallet, order.amount, &input.order_id, &self.orders, &self.audit).await
        } else if proof.starts_with("Voucher payment.") {
            // 3b. MB Voucher verification
            verify_voucher_proof(proof, &order, &input.order_id, &self.orders, &self.audit)
        } else {
            // Fallback: treat entire string as tx signature
            verify_on_chain_tx(&self.solana_rpc_url, proof, &self.merchant_wallet, order.amount, &input.order_id, &self.orders, &self.audit).await
        }
    }
}

// ── Payment verification helpers ──────────────────────────────────────────

async fn verify_on_chain_tx(
    rpc_url: &str,
    signature: &str,
    merchant_wallet: &str,
    expected_amount: u64,
    order_id: &str,
    orders: &PaymentOrderStore,
    audit: &AuditLogStore,
) -> String {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTransaction",
        "params": [signature, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]
    });

    let resp = match client.post(rpc_url).json(&body).send().await {
        Ok(r) => r,
        Err(e) => return format!("Error calling Solana RPC: {}", e),
    };

    let result: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return format!("Error parsing RPC response: {}", e),
    };

    // Check result is non-null
    let tx_info = match result.get("result") {
        Some(v) if !v.is_null() => v,
        _ => return format!("Transaction '{}' not found on-chain.", signature),
    };

    // Check meta.err is null (tx succeeded)
    if let Some(err) = tx_info["meta"]["err"].as_object() {
        return format!("Transaction failed: {:?}", err);
    }

    // Find merchant_wallet in account_keys and check balance change
    let account_keys = match tx_info["transaction"]["message"]["accountKeys"].as_array() {
        Some(keys) => keys,
        None => return "Error: Could not parse account_keys from transaction.".to_string(),
    };

    let wallet_idx = account_keys.iter().position(|k| {
        k.as_str() == Some(merchant_wallet)
    });

    let pre_balances = match tx_info["meta"]["preBalances"].as_array() {
        Some(b) => b,
        None => return "Error: Could not parse preBalances.".to_string(),
    };
    let post_balances = match tx_info["meta"]["postBalances"].as_array() {
        Some(b) => b,
        None => return "Error: Could not parse postBalances.".to_string(),
    };

    match wallet_idx {
        Some(idx) => {
            let pre = pre_balances[idx].as_u64().unwrap_or(0);
            let post = post_balances[idx].as_u64().unwrap_or(0);
            let received = post.saturating_sub(pre);
            if received >= expected_amount {
                if let Err(e) = orders.update_status(order_id, &ignite_pay_merchant_mcp::payment::OrderStatus::Confirmed) {
                    return format!("Payment verified but failed to update order: {}", e);
                }
                let _ = audit.append("payment_verified", Some(order_id), Some(received), "On-chain tx verified");
                format!(
                    "Payment verified.\nTx: {}\nMerchant received: {} lamports\nOrder: {} confirmed.",
                    signature, received, order_id
                )
            } else {
                format!(
                    "Payment amount mismatch. Expected {} lamports, received {} lamports.",
                    expected_amount, received
                )
            }
        }
        None => format!("Merchant wallet {} not found in transaction.", merchant_wallet),
    }
}

fn verify_voucher_proof(
    proof: &str,
    order: &PaymentOrder,
    order_id: &str,
    orders: &PaymentOrderStore,
    audit: &AuditLogStore,
) -> String {
    // Parse multi-line voucher format:
    // Voucher payment.
    // Channel: <base58>
    // Seq: <u64>
    // Amount: <u64>
    // Signature: <base58>
    // Message hash: <hex>
    let mut channel = String::new();
    let mut seq: Option<u64> = None;
    let mut amount: Option<u64> = None;
    let mut signature = String::new();
    let mut msg_hash = String::new();

    for line in proof.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("Channel: ") {
            channel = v.to_string();
        } else if let Some(v) = line.strip_prefix("Seq: ") {
            seq = v.parse().ok();
        } else if let Some(v) = line.strip_prefix("Amount: ") {
            amount = v.parse().ok();
        } else if let Some(v) = line.strip_prefix("Signature: ") {
            signature = v.to_string();
        } else if let Some(v) = line.strip_prefix("Message hash: ") {
            msg_hash = v.to_string();
        }
    }

    let voucher_amount = match amount {
        Some(a) => a,
        None => return "Error: Could not parse Amount from voucher proof.".to_string(),
    };

    // Verify amount matches order
    if voucher_amount < order.amount {
        return format!(
            "Voucher amount {} is less than order amount {}.",
            voucher_amount, order.amount
        );
    }

    // Verify msg_hash: SHA256(channel_id || seq.to_be_bytes() || amount.to_be_bytes())
    let channel_pk = match channel.parse::<Pubkey>() {
        Ok(p) => p,
        Err(_) => return "Error: Invalid channel pubkey in voucher.".to_string(),
    };
    let channel_id = channel_pk.to_bytes();
    let seq_val = seq.unwrap_or(0);

    let computed_hash = {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(&channel_id);
        hasher.update(&seq_val.to_be_bytes());
        hasher.update(&voucher_amount.to_be_bytes());
        let hash = hasher.finalize();
        hex::encode(hash)
    };

    if !msg_hash.is_empty() && computed_hash != msg_hash {
        return format!(
            "Message hash mismatch. Computed: {}, provided: {}.",
            computed_hash, msg_hash
        );
    }

    // Signature format check: must be valid base58 and 64 bytes
    if !signature.is_empty() {
        match bs58::decode(&signature).into_vec() {
            Ok(v) if v.len() == 64 => { /* valid signature format */ }
            _ => return "Error: Invalid signature format in voucher (must be 64 bytes base58).".to_string(),
        }
    }

    // All checks passed — confirm order
    if let Err(e) = orders.confirm_order(order_id, &channel, 0, seq_val) {
        return format!("Voucher verified but failed to confirm order: {}", e);
    }
    let _ = audit.append("payment_verified", Some(order_id), Some(voucher_amount), "MB voucher verified");

    format!(
        "Payment verified (MB voucher).\nChannel: {}\nSeq: {}\nAmount: {} lamports\nOrder: {} confirmed.",
        channel, seq_val, voucher_amount, order_id
    )
}

#[tool_handler]
impl ServerHandler for MerchantMcpServer {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo::new(
            ServerCapabilities::builder().enable_tools().build(),
        )
        .with_instructions(
            "Ignite Pay Merchant MCP — generate payment QR codes, receive MagicBlock payment channel \
             vouchers, batch settle, and manage orders. Use generate_payment_qr to create a payment \
             QR code, mb_receive_voucher to accept signed vouchers, mb_settle_batch to settle on-chain.",
        )
    }
}

// ── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    // Always log to file and stderr
    let log_dir = std::env::var("AUDIT_LOG_DIR").unwrap_or_else(|_| "./data/logs".to_string());
    let _ = std::fs::create_dir_all(&log_dir);
    let file_appender = tracing_appender::rolling::daily(&log_dir, "merchant-mcp.log");
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr.and(file_appender))
        .with_env_filter(env_filter)
        .init();

    let cfg = config::load_config()?;
    tracing::info!("Loaded merchant config: hub={}", cfg.merchant.hub_endpoint);

    let db = sled::open(&cfg.storage.path)?;
    tracing::info!("Database opened at {}", cfg.storage.path);

    let mediator = Arc::new(MerchantMediator::new(&cfg.mediator.ws_url, &db)?);

    mediator.connect(None).await?;
    tracing::info!("Merchant mediator connected");

    // Auto-display pairing QR if no merchant app is paired
    if mediator.paired_phone_did().await.is_none() {
        let qr_path = format!("{}/pairing_qr.svg", cfg.storage.path);
        match mediator.generate_invitation_qr_svg(&qr_path) {
            Ok(url) => {
                tracing::info!("Pairing QR saved to: {}", qr_path);
                tracing::info!("Invitation URL:\n{}", url);
                tracing::info!("Scan the QR image with Ignite Pay Merchant to pair.");

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
        tracing::info!("Merchant app already paired: {}", phone);
    }

    let orders = Arc::new(PaymentOrderStore::from_db(db.clone()));
    let audit = Arc::new(AuditLogStore::from_db(db.clone()));

    // MagicBlock: Initialize MB RPC, program ID, merchant keypair, and stores
    let mb_rpc = Arc::new(RpcClient::new(&cfg.magicblock.rpc_url));
    let mb_program_id: Pubkey = cfg.magicblock.program_id.parse()
        .map_err(|e| anyhow::anyhow!("Invalid MB program_id: {}", e))?;
    tracing::info!("MagicBlock RPC: {}, Program: {}", cfg.magicblock.rpc_url, mb_program_id);

    // Load or generate MB merchant keypair (persist in sled)
    let mb_keys_tree = db.open_tree("mb_keys")?;
    let mb_merchant_keypair = match mb_keys_tree.get("merchant_keypair")? {
        Some(bytes) => {
            if bytes.len() == 64 {
                let kp = MbKeypair::try_from(bytes.as_ref())
                    .map_err(|e| anyhow::anyhow!("Failed to load MB keypair: {}", e))?;
                tracing::info!("Loaded existing MB merchant keypair: {}", kp.pubkey());
                Arc::new(kp)
            } else {
                let kp = MbKeypair::new();
                mb_keys_tree.insert("merchant_keypair", kp.to_bytes().as_ref())?;
                mb_keys_tree.flush()?;
                tracing::info!("Generated new MB merchant keypair: {}", kp.pubkey());
                Arc::new(kp)
            }
        }
        None => {
            let kp = MbKeypair::new();
            mb_keys_tree.insert("merchant_keypair", kp.to_bytes().as_ref())?;
            mb_keys_tree.flush()?;
            tracing::info!("Generated new MB merchant keypair: {}", kp.pubkey());
            Arc::new(kp)
        }
    };

    let mb_voucher_store = Arc::new(MerchantVoucherStore::new(db.clone()));
    let mb_settlement_store = Arc::new(SettlementStore::new(db.clone()));

    let server = MerchantMcpServer {
        tool_router: MerchantMcpServer::tool_router(),
        mediator: mediator.clone(),
        orders: orders.clone(),
        audit: audit.clone(),
        mb_rpc: mb_rpc.clone(),
        mb_program_id,
        mb_merchant_keypair: mb_merchant_keypair.clone(),
        mb_voucher_store: mb_voucher_store.clone(),
        mb_settlement_store: mb_settlement_store.clone(),
        merchant_wallet: cfg.merchant.wallet.clone(),
        default_accept_tokens: cfg.merchant.accept_tokens.clone(),
        did_registry_url: cfg.did_registry.url.clone(),
        products: cfg.products.clone(),
        solana_rpc_url: cfg.solana.rpc_url.clone(),
    };

    // Set up MB voucher channel: mediator forwards received vouchers to this handler
    let (mb_voucher_tx, mut mb_voucher_rx) = tokio::sync::mpsc::unbounded_channel::<ignite_pay_merchant_mcp::mediator::MbVoucherCommand>();
    mediator.set_mb_voucher_channel(mb_voucher_tx).await;

    // Spawn handler for MB vouchers received via DIDComm
    {
        let mb_merchant_keypair = mb_merchant_keypair.clone();
        let mb_voucher_store = mb_voucher_store.clone();
        let orders = orders.clone();
        let mediator = mediator.clone();
        let mb_program_id_val = mb_program_id;
        tokio::spawn(async move {
            while let Some(cmd) = mb_voucher_rx.recv().await {
                tracing::info!("Processing MB voucher: buyer={}, order={}, seq={}", cmd.buyer_pubkey, cmd.order_id, cmd.seq);

                let buyer = match cmd.buyer_pubkey.parse::<Pubkey>() {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!("Invalid buyer pubkey in MB voucher: {}", e);
                        continue;
                    }
                };

                let buyer_sig = match bs58::decode(&cmd.buyer_sig).into_vec() {
                    Ok(v) if v.len() == 64 => {
                        let mut arr = [0u8; 64];
                        arr.copy_from_slice(&v);
                        arr
                    }
                    _ => {
                        tracing::error!("Invalid buyer_sig in MB voucher");
                        continue;
                    }
                };

                let token_mint = Pubkey::default();
                let (channel_pda, _) = pda::derive_channel_pda(
                    &mb_program_id_val,
                    &buyer,
                    &mb_merchant_keypair.pubkey(),
                    &token_mint,
                );
                let channel_id = channel_pda.to_bytes();

                // Verify signature
                let msg_hash = {
                    use sha2::{Sha256, Digest};
                    let mut hasher = Sha256::new();
                    hasher.update(&channel_id);
                    hasher.update(&cmd.seq.to_be_bytes());
                    hasher.update(&cmd.amount.to_be_bytes());
                    let mut hash = [0u8; 32];
                    hash.copy_from_slice(&hasher.finalize());
                    hash
                };

                if !signing::verify_signature(&buyer.to_bytes(), &msg_hash, &buyer_sig) {
                    tracing::error!("MB voucher signature verification failed for buyer {}", cmd.buyer_pubkey);
                    continue;
                }

                // Store voucher
                let voucher = ignite_pay_merchant_mcp::voucher_store::CollectedVoucher {
                    channel_id,
                    buyer: buyer.to_bytes(),
                    seq: cmd.seq,
                    amount: cmd.amount,
                    buyer_sig,
                };

                if let Err(e) = mb_voucher_store.store_voucher(&voucher) {
                    tracing::error!("Failed to store MB voucher: {}", e);
                    continue;
                }

                // Confirm order
                if !cmd.order_id.is_empty() {
                    if let Err(e) = orders.confirm_order(&cmd.order_id, &channel_pda.to_string(), 0, cmd.seq) {
                        tracing::warn!("Failed to confirm order {}: {}", cmd.order_id, e);
                    }
                }

                // Send payment confirmation to merchant app
                if !cmd.order_id.is_empty() {
                    if let Some(phone_did) = mediator.paired_phone_did().await {
                        if let Err(e) = mediator.send_payment_confirmation(
                            &phone_did,
                            &cmd.order_id,
                            &channel_pda.to_string(),
                            0,
                            cmd.seq,
                        ).await {
                            tracing::warn!("Failed to send payment confirmation to app: {}", e);
                        }
                    }
                }

                tracing::info!("MB voucher processed successfully: channel={}, seq={}", channel_pda, cmd.seq);
            }
        });
    }

    tracing::info!("Starting merchant MCP server on stdio...");
    // Spawn the MCP stdio server in a background task (non-fatal if no client connects).
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
    if cfg.mcp.sse_port > 0 {
        let sse_port = cfg.mcp.sse_port;
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
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down...");

    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_pay_merchant_mcp::payment::OrderStatus;
    use sha2::{Digest, Sha256};

    fn setup_stores() -> (tempfile::TempDir, PaymentOrderStore, AuditLogStore) {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let orders = PaymentOrderStore::from_db(db.clone());
        let audit = AuditLogStore::from_db(db);
        (dir, orders, audit)
    }

    fn make_order(id: &str, amount: u64) -> PaymentOrder {
        PaymentOrder {
            order_id: id.to_string(),
            merchant_did: "did:ignite:zTest".to_string(),
            amount,
            description: "Test order".to_string(),
            hub_endpoint: "http://localhost:3003".to_string(),
            status: OrderStatus::Pending,
            created_at: chrono::Utc::now(),
            confirmed_at: None,
            channel_id: None,
            leaf_index: None,
            sequence: None,
        }
    }

    fn compute_msg_hash(channel_id: &[u8; 32], seq: u64, amount: u64) -> String {
        let mut hasher = Sha256::new();
        hasher.update(channel_id);
        hasher.update(&seq.to_be_bytes());
        hasher.update(&amount.to_be_bytes());
        hex::encode(hasher.finalize())
    }

    // ── Voucher proof tests ──────────────────────────────────────────

    #[test]
    fn test_voucher_proof_valid() {
        let (_dir, orders, audit) = setup_stores();
        let order = make_order("v-1", 100_000);
        orders.save_order(&order).unwrap();

        let channel_pk = Pubkey::new_unique();
        let seq: u64 = 1;
        let amount: u64 = 100_000;
        let msg_hash = compute_msg_hash(&channel_pk.to_bytes(), seq, amount);
        let sig = bs58::encode([1u8; 64]).into_string();
        let proof = format!(
            "Voucher payment.\nChannel: {}\nSeq: {}\nAmount: {}\nSignature: {}\nMessage hash: {}",
            channel_pk, seq, amount, sig, msg_hash
        );

        let result = verify_voucher_proof(&proof, &order, "v-1", &orders, &audit);
        assert!(result.contains("Payment verified (MB voucher)"));
        assert!(result.contains("confirmed"));

        let loaded = orders.get_order("v-1").unwrap().unwrap();
        assert_eq!(loaded.status, OrderStatus::Confirmed);
        assert_eq!(loaded.channel_id.as_deref(), Some(channel_pk.to_string().as_str()));
        assert_eq!(loaded.sequence, Some(seq));
    }

    #[test]
    fn test_voucher_proof_amount_greater_than_order() {
        let (_dir, orders, audit) = setup_stores();
        // Order is 50_000, voucher pays 100_000 — should pass
        let order = make_order("v-overpay", 50_000);
        orders.save_order(&order).unwrap();

        let channel_pk = Pubkey::new_unique();
        let amount: u64 = 100_000;
        let msg_hash = compute_msg_hash(&channel_pk.to_bytes(), 1, amount);
        let sig = bs58::encode([2u8; 64]).into_string();
        let proof = format!(
            "Voucher payment.\nChannel: {}\nSeq: 1\nAmount: {}\nSignature: {}\nMessage hash: {}",
            channel_pk, amount, sig, msg_hash
        );

        let result = verify_voucher_proof(&proof, &order, "v-overpay", &orders, &audit);
        assert!(result.contains("Payment verified (MB voucher)"));
    }

    #[test]
    fn test_voucher_proof_amount_too_low() {
        let (_dir, orders, audit) = setup_stores();
        let order = make_order("v-low", 200_000);

        let channel_pk = Pubkey::new_unique();
        let sig = bs58::encode([1u8; 64]).into_string();
        let proof = format!(
            "Voucher payment.\nChannel: {}\nSeq: 1\nAmount: 100000\nSignature: {}\nMessage hash: abc",
            channel_pk, sig
        );

        let result = verify_voucher_proof(&proof, &order, "v-low", &orders, &audit);
        assert!(result.contains("less than order amount"));
    }

    #[test]
    fn test_voucher_proof_msg_hash_mismatch() {
        let (_dir, orders, audit) = setup_stores();
        let order = make_order("v-hash", 100_000);

        let channel_pk = Pubkey::new_unique();
        let sig = bs58::encode([1u8; 64]).into_string();
        let proof = format!(
            "Voucher payment.\nChannel: {}\nSeq: 1\nAmount: 100000\nSignature: {}\nMessage hash: deadbeef",
            channel_pk, sig
        );

        let result = verify_voucher_proof(&proof, &order, "v-hash", &orders, &audit);
        assert!(result.contains("hash mismatch"));
    }

    #[test]
    fn test_voucher_proof_invalid_channel() {
        let (_dir, orders, audit) = setup_stores();
        let order = make_order("v-badch", 100_000);

        let sig = bs58::encode([1u8; 64]).into_string();
        let proof = format!(
            "Voucher payment.\nChannel: NOT_VALID_BASE58!!!\nSeq: 1\nAmount: 100000\nSignature: {}\nMessage hash: abc",
            sig
        );

        let result = verify_voucher_proof(&proof, &order, "v-badch", &orders, &audit);
        assert!(result.contains("Invalid channel pubkey"));
    }

    #[test]
    fn test_voucher_proof_invalid_signature_format() {
        let (_dir, orders, audit) = setup_stores();
        let order = make_order("v-badsig", 100_000);

        let channel_pk = Pubkey::new_unique();
        let msg_hash = compute_msg_hash(&channel_pk.to_bytes(), 1, 100_000);
        // 32-byte sig instead of 64
        let short_sig = bs58::encode([1u8; 32]).into_string();
        let proof = format!(
            "Voucher payment.\nChannel: {}\nSeq: 1\nAmount: 100000\nSignature: {}\nMessage hash: {}",
            channel_pk, short_sig, msg_hash
        );

        let result = verify_voucher_proof(&proof, &order, "v-badsig", &orders, &audit);
        assert!(result.contains("Invalid signature format"));
    }

    #[test]
    fn test_voucher_proof_missing_amount() {
        let (_dir, orders, audit) = setup_stores();
        let order = make_order("v-noamt", 100_000);

        let proof = "Voucher payment.\nChannel: 11111111111111111111111111111111\nSeq: 1\nSignature: abc";
        let result = verify_voucher_proof(proof, &order, "v-noamt", &orders, &audit);
        assert!(result.contains("Could not parse Amount"));
    }

    // ── On-chain tx verification tests ───────────────────────────────

    #[tokio::test]
    async fn test_on_chain_tx_valid() {
        let mut server = mockito::Server::new_async().await;
        let (_dir, orders, audit) = setup_stores();
        let order = make_order("tx-1", 100_000);
        orders.save_order(&order).unwrap();

        let wallet = "MerchantW1111111111111111111111111111111111111";
        let other = "Other11111111111111111111111111111111111111111";
        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "result": {
                "meta": { "err": null, "preBalances": [500_000, 1_000_000], "postBalances": [600_000, 1_000_000] },
                "transaction": { "message": { "accountKeys": [wallet, other] } }
            }
        });

        let mock = server.mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body.to_string())
            .create_async()
            .await;

        let result = verify_on_chain_tx(server.url().as_str(), "testsig", wallet, 100_000, "tx-1", &orders, &audit).await;
        assert!(result.contains("Payment verified"));
        assert!(result.contains("100000 lamports"));
        assert!(result.contains("confirmed"));
        mock.assert_async().await;

        let loaded = orders.get_order("tx-1").unwrap().unwrap();
        assert_eq!(loaded.status, OrderStatus::Confirmed);
    }

    #[tokio::test]
    async fn test_on_chain_tx_not_found() {
        let mut server = mockito::Server::new_async().await;
        let (_dir, orders, audit) = setup_stores();

        let body = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": null });
        server.mock("POST", "/")
            .with_status(200)
            .with_body(body.to_string())
            .create_async()
            .await;

        let result = verify_on_chain_tx(server.url().as_str(), "badsig", "wallet", 100_000, "tx-nf", &orders, &audit).await;
        assert!(result.contains("not found on-chain"));
    }

    #[tokio::test]
    async fn test_on_chain_tx_failed() {
        let mut server = mockito::Server::new_async().await;
        let (_dir, orders, audit) = setup_stores();

        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "result": {
                "meta": { "err": { "InstructionError": [0, "Custom(1)"] } },
                "transaction": { "message": { "accountKeys": [] } }
            }
        });
        server.mock("POST", "/")
            .with_status(200)
            .with_body(body.to_string())
            .create_async()
            .await;

        let result = verify_on_chain_tx(server.url().as_str(), "failesig", "wallet", 100_000, "tx-fail", &orders, &audit).await;
        assert!(result.contains("Transaction failed"));
    }

    #[tokio::test]
    async fn test_on_chain_tx_wallet_not_in_tx() {
        let mut server = mockito::Server::new_async().await;
        let (_dir, orders, audit) = setup_stores();

        let other = "Other11111111111111111111111111111111111111111";
        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "result": {
                "meta": { "err": null, "preBalances": [1_000_000], "postBalances": [900_000] },
                "transaction": { "message": { "accountKeys": [other] } }
            }
        });
        server.mock("POST", "/")
            .with_status(200)
            .with_body(body.to_string())
            .create_async()
            .await;

        let result = verify_on_chain_tx(server.url().as_str(), "sig", "MissingWallet11111111111111111111111", 100_000, "tx-nowallet", &orders, &audit).await;
        assert!(result.contains("not found in transaction"));
    }

    #[tokio::test]
    async fn test_on_chain_tx_insufficient_amount() {
        let mut server = mockito::Server::new_async().await;
        let (_dir, orders, audit) = setup_stores();

        let wallet = "MerchantW1111111111111111111111111111111111111";
        let other = "Other11111111111111111111111111111111111111111";
        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "result": {
                "meta": { "err": null, "preBalances": [500_000, 1_000_000], "postBalances": [550_000, 1_000_000] },
                "transaction": { "message": { "accountKeys": [wallet, other] } }
            }
        });
        server.mock("POST", "/")
            .with_status(200)
            .with_body(body.to_string())
            .create_async()
            .await;

        let result = verify_on_chain_tx(server.url().as_str(), "sig", wallet, 100_000, "tx-low", &orders, &audit).await;
        assert!(result.contains("amount mismatch"));
        assert!(result.contains("50000"));
    }

    #[tokio::test]
    async fn test_on_chain_tx_rpc_error() {
        let mut server = mockito::Server::new_async().await;
        let (_dir, orders, audit) = setup_stores();

        server.mock("POST", "/")
            .with_status(500)
            .create_async()
            .await;

        let result = verify_on_chain_tx(server.url().as_str(), "sig", "wallet", 100_000, "tx-err", &orders, &audit).await;
        assert!(result.contains("Error calling Solana RPC") || result.contains("Error parsing RPC response"));
    }
}
