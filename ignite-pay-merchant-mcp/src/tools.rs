use schemars::JsonSchema;
use serde::Deserialize;

/// Input for the `generate_payment_qr` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GeneratePaymentQrInput {
    /// Payment amount in smallest token units (e.g., lamports for SOL).
    pub amount: u64,
    /// Description of the payment (e.g., "Coffee").
    #[serde(default)]
    pub description: String,
    /// Optional order ID. If not provided, a UUID will be generated.
    #[serde(default)]
    pub order_id: Option<String>,
    /// Override the default accept_tokens list from config.
    #[serde(default)]
    pub accept_tokens: Option<Vec<String>>,
}

/// Input for the `check_payment` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CheckPaymentInput {
    /// Order ID to check.
    pub order_id: String,
}

/// Input for the `get_payment_history` tool.
fn default_limit() -> usize {
    20
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetPaymentHistoryInput {
    /// Maximum number of records to return (default 20).
    #[serde(default = "default_limit")]
    pub limit: usize,
}

// MagicBlock payment channel tools

/// Input for the `mb_get_channel` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MbGetChannelInput {
    /// Buyer pubkey (base58).
    pub buyer_pubkey: String,
}

/// Input for the `mb_receive_voucher` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MbReceiveVoucherInput {
    /// Buyer pubkey (base58).
    pub buyer_pubkey: String,
    /// Voucher sequence number.
    pub seq: u64,
    /// Payment amount in lamports.
    pub amount: u64,
    /// Buyer's Ed25519 signature on the voucher (base58).
    pub buyer_sig: String,
    /// Optional order ID. If provided, the corresponding order will be confirmed automatically.
    #[serde(default)]
    pub order_id: Option<String>,
}

/// Input for the `mb_settle_batch` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MbSettleBatchInput {
    /// Buyer pubkey (base58).
    pub buyer_pubkey: String,
    /// Buyer's batch settlement signature (base58).
    pub buyer_batch_sig: String,
}

/// Input for the `mb_optimistic_settle` tool (merchant-only, no buyer signature needed).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MbOptimisticSettleInput {
    /// Buyer pubkey (base58).
    pub buyer_pubkey: String,
}

/// Input for the `mb_get_settlement` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MbGetSettlementInput {
    /// Buyer pubkey (base58).
    pub buyer_pubkey: String,
    /// Batch nonce.
    pub batch_nonce: u64,
}

/// Input for the `mb_release_settlement` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MbReleaseSettlementInput {
    /// Buyer pubkey (base58).
    pub buyer_pubkey: String,
    /// Batch nonce.
    pub batch_nonce: u64,
}

/// Input for the `mb_force_release` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MbForceReleaseInput {
    /// Buyer pubkey (base58).
    pub buyer_pubkey: String,
    /// Batch nonce.
    pub batch_nonce: u64,
}

// DID Registry tools

/// Input for the `register_merchant` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RegisterMerchantInput {
    /// Merchant display name.
    pub merchant_name: String,
    /// Merchant category (e.g., "retail", "food").
    #[serde(default)]
    pub category: Option<String>,
}

/// Input for the `verify_merchant_did` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct VerifyMerchantDidInput {}

// Agent x402 payment tools

/// Input for the `list_products` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListProductsInput {}

/// Input for the `create_order` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateOrderInput {
    /// Product ID from list_products (optional, if provided uses product price).
    #[serde(default)]
    pub product_id: Option<String>,
    /// Amount in lamports (required if no product_id).
    #[serde(default)]
    pub amount: Option<u64>,
    /// Order description (optional).
    #[serde(default)]
    pub description: Option<String>,
}

/// Input for the `verify_payment` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct VerifyPaymentInput {
    /// Order ID returned by create_order.
    pub order_id: String,
    /// Payment proof string from buyer MCP (e.g. "Tx: <base58_sig>" or voucher format).
    pub payment_proof: String,
}
