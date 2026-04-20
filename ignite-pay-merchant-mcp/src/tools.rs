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

/// Input for the `get_channel_status` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetChannelStatusInput {
    /// Channel ID (hex string). If empty, lists all channels.
    #[serde(default)]
    pub channel_id: Option<String>,
}

/// Input for the `open_channel_with_hub` tool.
fn default_tree_depth() -> u32 {
    8
}

fn default_deposit() -> u64 {
    0
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpenChannelWithHubInput {
    /// Hub HTTP endpoint URL.
    pub hub_endpoint: String,
    /// Amount to deposit as provider collateral.
    #[serde(default = "default_deposit")]
    pub deposit: u64,
    /// Merkle tree depth (default 8).
    #[serde(default = "default_tree_depth")]
    pub tree_depth: u32,
}

/// Input for the `close_channel` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloseChannelInput {
    /// Channel ID (hex string).
    pub channel_id: String,
}

/// Input for the `settle_channel` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SettleChannelInput {
    /// Channel ID (hex string).
    pub channel_id: String,
}
