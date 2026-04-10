use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

/// A leaf node in the Concurrent Merkle Tree storing merchant identity.
#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub struct MerchantLeaf {
    /// SHA-256 hash of the DID string
    pub merchant_did: [u8; 32],
    /// Current active receiving public key
    pub active_pubkey: Pubkey,
    /// SHA-256 hash of the platform Verifiable Credential
    pub platform_vc_hash: [u8; 32],
    /// Slot when this leaf was last updated
    pub slot_updated: u64,
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

/// Merkle proof for a compressed leaf.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    /// Index of the leaf in the tree
    pub leaf_index: u32,
    /// Sibling hashes from leaf to root
    pub proof: Vec<[u8; 32]>,
    /// Current tree root hash
    pub root: [u8; 32],
    /// Hash of the leaf
    pub leaf_hash: [u8; 32],
}

/// Result of merchant on-chain verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerchantVerification {
    /// Whether the merchant is verified on-chain
    pub verified: bool,
    /// The merchant leaf data
    pub leaf: MerchantLeaf,
    /// The Merkle proof
    pub proof: MerkleProof,
}
