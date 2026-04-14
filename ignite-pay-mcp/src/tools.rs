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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_x402_challenge_input_minimal() {
        let json = r#"{"challenge_body":"{\"accepts\":[]}","phone_did":"did:ignite:zPhone"}"#;
        let input: X402ChallengeInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.challenge_body, "{\"accepts\":[]}");
        assert_eq!(input.phone_did, "did:ignite:zPhone");
        assert!(input.x402_merchant_did.is_none());
        assert!(input.x402_payment_address.is_none());
        assert!(input.vc_ipfs_cid.is_none());
    }

    #[test]
    fn test_x402_challenge_input_full() {
        let json = r#"{
            "challenge_body": "{\"accepts\":[{\"amount\":\"100\"}]}",
            "phone_did": "did:ignite:zPhone",
            "x402_merchant_did": "did:ignite:zMerchant",
            "x402_payment_address": "wallet123",
            "x402_merkle_context": "{\"leaf_index\":5}",
            "vc_ipfs_cid": "bafyreiTest"
        }"#;
        let input: X402ChallengeInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.x402_merchant_did.as_deref(), Some("did:ignite:zMerchant"));
        assert_eq!(input.x402_payment_address.as_deref(), Some("wallet123"));
        assert_eq!(input.x402_merkle_context.as_deref(), Some("{\"leaf_index\":5}"));
        assert_eq!(input.vc_ipfs_cid.as_deref(), Some("bafyreiTest"));
    }

    #[test]
    fn test_payment_history_default_limit() {
        let json = r#"{}"#;
        let input: PaymentHistoryInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.limit, 10);
    }

    #[test]
    fn test_payment_history_custom_limit() {
        let json = r#"{"limit": 50}"#;
        let input: PaymentHistoryInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.limit, 50);
    }

    #[test]
    fn test_create_session_default_duration() {
        let json = r#"{"owner_pubkey":"11111111111111111111111111111111","spending_limit":1000}"#;
        let input: CreateSessionInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.duration_secs, 3600);
        assert_eq!(input.spending_limit, 1000);
    }

    #[test]
    fn test_create_session_custom_duration() {
        let json = r#"{"owner_pubkey":"11111111111111111111111111111111","spending_limit":5000,"duration_secs":7200}"#;
        let input: CreateSessionInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.duration_secs, 7200);
    }

    #[test]
    fn test_close_session_no_refund() {
        let json = r#"{"session_pubkey":"sess_pub","owner_pubkey":"owner_pub"}"#;
        let input: CloseSessionInput = serde_json::from_str(json).unwrap();
        assert!(!input.refund);
    }

    #[test]
    fn test_close_session_with_refund() {
        let json = r#"{"session_pubkey":"sess_pub","owner_pubkey":"owner_pub","refund":true}"#;
        let input: CloseSessionInput = serde_json::from_str(json).unwrap();
        assert!(input.refund);
    }
}
