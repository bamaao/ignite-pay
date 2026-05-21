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
use solana_sdk::pubkey::Pubkey;

/// Compressed merchant DID account data (mirrors the on-chain struct).
/// This data lives as a compressed account hash in a Light Protocol state tree,
/// not as a traditional on-chain account. No discriminator or rent needed.
#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub struct MerchantDidAccount {
    /// Initial anchor public key (immutable).
    pub original_pk: Pubkey,
    /// Current controller public key.
    pub controller_pk: Pubkey,
    /// Recovery public key.
    pub recovery_pk: Pubkey,
    /// Platform verifiable credential hash.
    pub vc_hash: [u8; 32],
    /// Last update timestamp (Unix seconds).
    pub last_updated: i64,
    /// Anti-replay counter.
    pub nonce: u64,
}

/// Session token data persisted locally.
#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub struct SessionTokenData {
    /// Owner (payer) public key
    pub owner: Pubkey,
    /// Ephemeral signer public key
    pub ephemeral_signer: Pubkey,
    /// Target program (e.g., System Program or Token Program)
    pub target_program: Pubkey,
    /// SPL Token mint. Pubkey::default() for SOL sessions.
    pub token_mint: Pubkey,
    /// Unix timestamp when session expires
    pub expires_at: i64,
    /// Maximum spending limit in lamports
    pub spending_limit: u64,
    /// Cumulative amount spent so far
    pub current_spent: u64,
    /// Per-transaction spending limit in lamports. 0 = no limit.
    pub per_tx_limit: u64,
    /// Daily transaction count limit. 0 = no limit.
    pub daily_tx_count_limit: u32,
    /// Permission scopes (e.g., ["sol:transfer", "spl:transfer"])
    pub scopes: Vec<String>,
    /// Number of transactions executed today (local tracking).
    #[serde(default)]
    pub current_daily_count: u32,
    /// Unix timestamp of the start of the current daily counting window.
    /// Resets `current_daily_count` when a new day begins.
    #[serde(default)]
    pub last_daily_reset: i64,
}

/// Payment mode for the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PayMode {
    /// User pre-funds the ephemeral key, which pays directly
    SelfFunded,
    /// Project relayer pays gas; ephemeral key partial-signs
    Sponsored,
}

/// On-chain submission mode for DID operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnchainMode {
    /// Platform signs and sends the transaction, recording a service fee.
    #[default]
    Sponsored,
    /// Platform builds an unsigned transaction returned to the merchant for self-signing.
    SelfOnchain,
}

/// Result of a successful on-chain payment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentResult {
    /// Transaction signature (base58)
    pub signature: String,
    /// Slot in which the transaction was confirmed
    pub slot: u64,
    /// Block time (unix timestamp) if available
    pub block_time: Option<i64>,
}

/// Parameters for SPL Token payments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplPaymentParams {
    /// SPL Token mint address
    pub mint: Pubkey,
    /// Override source ATA (if None, derived from owner + mint)
    pub source_ata_override: Option<Pubkey>,
    /// Override destination ATA (if None, derived from recipient + mint)
    pub dest_ata_override: Option<Pubkey>,
}
