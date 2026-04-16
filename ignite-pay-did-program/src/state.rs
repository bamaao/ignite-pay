use anchor_lang::prelude::*;

/// Status constants for merchant DID leaves.
pub const MERCHANT_STATUS_ACTIVE: u8 = 0;
pub const MERCHANT_STATUS_SUSPENDED: u8 = 1;
pub const MERCHANT_STATUS_REVOKED: u8 = 2;

/// PDA holding global DID program configuration.
/// Seeds: [b"did-config"]
/// Space: 8 (disc) + 32 (platform_authority) + 32 (merkle_tree) + 1 (bump) = 73
#[account]
pub struct DidConfig {
    /// Platform authority that can update_vc and update_status.
    pub platform_authority: Pubkey,
    /// Address of the Concurrent Merkle Tree account.
    pub merkle_tree: Pubkey,
    /// PDA bump seed.
    pub bump: u8,
}

impl DidConfig {
    pub const LEN: usize = 8 + 32 + 32 + 1; // 73
}

/// Hash a byte slice using SHA-256.
pub fn hash_bytes(data: &[u8]) -> [u8; 32] {
    solana_sha256_hasher::hash(data).to_bytes()
}

/// Hash a pair of 32-byte values using SHA-256 (hashv equivalent).
pub fn hash_pair(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    solana_sha256_hasher::hashv(&[left, right]).to_bytes()
}

/// Compute the leaf hash for a merchant DID leaf node.
/// Must match `ignite-pay-solana/src/compression.rs:54` exactly:
/// `hashv([did_hash, pubkey_bytes, vc_hash, [status], slot_le_bytes])`
pub fn compute_merchant_leaf_hash(
    merchant_did_hash: &[u8; 32],
    active_pubkey: &Pubkey,
    platform_vc_hash: &[u8; 32],
    status: u8,
    slot_updated: u64,
) -> [u8; 32] {
    let active_pubkey_bytes = active_pubkey.to_bytes();
    let slot_bytes = slot_updated.to_le_bytes();
    let status_bytes = [status];
    solana_sha256_hasher::hashv(&[
        merchant_did_hash,
        &active_pubkey_bytes,
        platform_vc_hash,
        &status_bytes,
        &slot_bytes,
    ])
    .to_bytes()
}

/// Compute the Anchor instruction discriminator: sha256("global:<name>")[..8]
pub fn anchor_sighash(name: &str) -> [u8; 8] {
    let preimage = format!("global:{}", name);
    let h = solana_sha256_hasher::hash(preimage.as_bytes());
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&h.to_bytes()[..8]);
    disc
}
