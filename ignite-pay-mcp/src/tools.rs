use schemars::JsonSchema;
use serde::Deserialize;

/// Input for the `process_x402_challenge` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct X402ChallengeInput {
    /// The HTTP 402 response body (JSON string with "accepts" array).
    pub challenge_body: String,
    /// Recipient phone DID for authorization.
    pub phone_did: String,
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
