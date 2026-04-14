use schemars::JsonSchema;
use serde::Deserialize;

/// Input for the `process_x402_challenge` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct X402ChallengeInput {
    /// The HTTP 402 response body (JSON string with "accepts" array).
    pub challenge_body: String,
    /// Recipient phone DID for authorization.
    pub phone_did: String,
    // V1.1: X402 extended headers
    /// Merchant DID from x402-merchant-did header (overrides provider_did from body).
    pub x402_merchant_did: Option<String>,
    /// Payment address from x402-payment-address header.
    pub x402_payment_address: Option<String>,
    /// Merkle context from x402-merkle-context header (JSON with leaf_index, proof_nodes, root_index).
    pub x402_merkle_context: Option<String>,
    /// IPFS CID for a Verifiable Credential endorsing the merchant.
    pub vc_ipfs_cid: Option<String>,
}

/// Input for the `check_authorization` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AuthorizationCheckInput {
    /// Payment request ID to check.
    pub payment_id: String,
}

/// Input for the `get_payment_history` tool.
fn default_limit() -> usize {
    10
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PaymentHistoryInput {
    /// Maximum number of records to return (default 10).
    #[serde(default = "default_limit")]
    pub limit: usize,
}

// V2.0: Session key management tools

/// Input for the `create_session` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateSessionInput {
    /// Owner (payer) Solana public key (base58).
    pub owner_pubkey: String,
    /// Maximum spending limit in lamports.
    pub spending_limit: u64,
    /// Session duration in seconds (default 3600 = 1 hour).
    #[serde(default = "default_duration")]
    pub duration_secs: i64,
}

fn default_duration() -> i64 {
    3600
}

/// Input for the `get_session_status` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionStatusInput {
    /// Owner (payer) Solana public key (base58).
    pub owner_pubkey: String,
}

/// Input for the `close_session` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloseSessionInput {
    /// Session key public key to close (base58).
    pub session_pubkey: String,
    /// Owner public key for refund (base58).
    pub owner_pubkey: String,
    /// Whether to refund remaining SOL to owner before closing.
    #[serde(default)]
    pub refund: bool,
}
