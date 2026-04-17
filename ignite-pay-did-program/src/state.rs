use anchor_lang::prelude::*;
use light_sdk::LightDiscriminator;

/// Compressed merchant DID account stored via ZK Compression.
/// Uses `#[event]` (not `#[account]`) so Anchor includes it in the IDL
/// without allocating on-chain account space. The actual data lives as
/// a compressed account hash in a Light Protocol state tree.
#[event]
#[derive(Clone, Debug, Default, LightDiscriminator)]
pub struct MerchantCompressedDid {
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
