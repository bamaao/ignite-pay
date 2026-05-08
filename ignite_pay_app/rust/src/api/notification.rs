// Copyright (c) 2026 zouyc zouyccq@gmail.com.
// All rights reserved.
//
// Licensed under the Business Source License 1.1 (BSL 1.1).
// You may not use this file except in compliance with the License.
//
// Change Date: 2031-01-01
// On the Change Date, or the fourth anniversary of the first publicly available
// distribution of the code under the BSL, whichever comes first, the code
// automatically becomes available under the Apache License 2.0.

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
    // F2: MCP-provided new session key info for embedded payment flow.
    /// Base58-encoded ephemeral public key from MCP.
    pub new_session_key_pubkey: Option<String>,
    /// Base58-encoded 64-byte ephemeral keypair (secret key) from MCP.
    pub new_session_key_secret_key: Option<String>,
    /// Spending limit in lamports.
    pub new_session_key_spending_limit: Option<u64>,
    /// Session duration in seconds.
    pub new_session_key_duration_secs: Option<i64>,
    /// Permission scopes.
    pub new_session_key_scopes: Option<Vec<String>>,
    /// SPL Token mint address (base58).
    pub new_session_key_token_mint: Option<String>,
    /// Suggested SOL funding amount (lamports).
    pub new_session_key_suggested_sol_funding: Option<u64>,
    /// Suggested token funding amount.
    pub new_session_key_suggested_token_funding: Option<u64>,
    /// Available payment methods (e.g., ["session_key", "magicblock"]).
    pub available_payment_methods: Option<Vec<String>>,
    // F3/F7: Session fund request fields
    pub session_fund_required_amount: Option<u64>,
    pub session_fund_current_balance: Option<u64>,
    pub session_fund_spending_limit_remaining: Option<u64>,
    pub session_fund_token_mint: Option<String>,
    pub session_fund_reason: Option<String>,
    // F13: Balance notification fields
    pub balance_notification_balance: Option<u64>,
    pub balance_notification_threshold: Option<u64>,
    pub balance_notification_spending_limit_remaining: Option<u64>,
    // F14: Session renew request fields
    pub old_session_key_pubkey: Option<String>,
    pub session_renew_expires_at: Option<i64>,
    // F16: Relayer payment method fields
    pub relayer_pubkey: Option<String>,
    pub relayer_url: Option<String>,
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
