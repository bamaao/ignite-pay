use ignite_pay_merchant_mcp::audit::AuditLogStore;
use ignite_pay_merchant_mcp::config;
use ignite_pay_merchant_mcp::mediator::MerchantMediator;
use ignite_pay_merchant_mcp::payment::{PaymentOrder, PaymentOrderStore};
use ignite_pay_merchant_mcp::qr::{self, PaymentQrData};
use ignite_pay_merchant_mcp::settlement_store::SettlementStore;
use ignite_pay_merchant_mcp::tools::{
    CheckPaymentInput, GeneratePaymentQrInput, GetPaymentHistoryInput,
    MbForceReleaseInput, MbGetChannelInput, MbGetSettlementInput,
    MbOptimisticSettleInput, MbReceiveVoucherInput, MbReleaseSettlementInput,
    MbSettleBatchInput,
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
