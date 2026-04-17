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

/// On-chain PDA storing the platform's Ed25519 public key.
/// Seeds: [b"platform-config"]. Initialized once via `init_platform`.
#[account]
pub struct PlatformConfig {
    /// Platform Ed25519 public key (32 bytes).
    pub platform_ed25519_pubkey: [u8; 32],
    /// Authority that can update the platform key (upgrade authority).
    pub authority: Pubkey,
    /// PDA bump seed.
    pub bump: u8,
}

/// On-chain revocation registry entry. Each revoked VC gets its own PDA.
/// Seeds: [b"revoked-vc", vc_hash]. Created via `revoke_vc`.
/// Verifiers check PDA existence to determine if a VC has been revoked.
#[account]
pub struct RevokedVc {
    /// The revoked VC hash.
    pub vc_hash: [u8; 32],
    /// The credential subject's public key.
    pub credential_subject_pk: Pubkey,
    /// Revocation timestamp (Unix seconds).
    pub revoked_at: i64,
    /// Revocation reason (0=unspecified, 1=violation, 2=expired, etc.).
    pub reason: u8,
    /// Authority that performed the revocation (must match PlatformConfig.authority).
    pub authority: Pubkey,
    /// PDA bump seed.
    pub bump: u8,
}
