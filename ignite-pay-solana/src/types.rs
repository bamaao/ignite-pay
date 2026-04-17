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
    /// Unix timestamp when session expires
    pub expires_at: i64,
    /// Maximum spending limit in lamports
    pub spending_limit: u64,
    /// Cumulative amount spent so far
    pub current_spent: u64,
    /// Permission scopes (e.g., ["sol:transfer", "spl:transfer"])
    pub scopes: Vec<String>,
}

/// Payment mode for the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PayMode {
    /// User pre-funds the ephemeral key, which pays directly
    SelfFunded,
    /// Project relayer pays gas; ephemeral key partial-signs
    Sponsored,
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
