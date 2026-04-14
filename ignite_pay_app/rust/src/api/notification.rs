use serde::{Deserialize, Serialize};

/// A decrypted DIDComm message exposed to Flutter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptedMessage {
    /// Message type URI (e.g. "https://didcomm.org/ignite-pay/1.0/payment-auth-request")
    pub msg_type: String,
    /// Payment ID for authorization flows.
    pub payment_id: Option<String>,
    /// DID of the merchant requesting payment.
    pub merchant_did: Option<String>,
    /// Amount in lamports.
    pub amount: Option<u64>,
    /// Human-readable description of the payment.
    pub description: Option<String>,
    /// IPFS CID for list sync notifications.
    pub list_cid: Option<String>,
    /// Action type for list sync (add/remove).
    pub action: Option<String>,
    /// Target DID for list operations.
    pub target_did: Option<String>,
    /// Raw message body as JSON string.
    pub raw_body: String,
    // V1.1:
    /// List type from list-sync-notification ("whitelist" or "blacklist").
    pub list_type: Option<String>,
    /// User-assigned label for the merchant.
    pub label: Option<String>,
}

/// A DIDComm message envelope (before decryption).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DidcommMessage {
    /// Unique message ID.
    pub msg_id: String,
    /// JWE encrypted envelope.
    pub jwe_envelope: String,
    /// Unix timestamp when the message was created.
    pub created_at: i64,
}
