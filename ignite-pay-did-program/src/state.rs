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

use anchor_lang::prelude::*;

// ─── PDA version (default) ─────────────────────────────────────────────
/// On-chain PDA storing merchant DID data.
/// Seeds: [b"merchant-did", original_pk]. Created via `initialize_did`.
#[cfg(not(feature = "zk-compression"))]
#[account]
pub struct MerchantDidAccount {
    /// Initial anchor public key (immutable).
    pub original_pk: Pubkey,     // 32
    /// Current controller public key.
    pub controller_pk: Pubkey,   // 32
    /// Recovery public key.
    pub recovery_pk: Pubkey,     // 32
    /// Platform verifiable credential hash.
    pub vc_hash: [u8; 32],       // 32
    /// Last update timestamp (Unix seconds).
    pub last_updated: i64,       // 8
    /// Anti-replay counter.
    pub nonce: u64,              // 8
    /// PDA bump seed.
    pub bump: u8,                // 1
}
// Space = 8(discriminator) + 32 + 32 + 32 + 32 + 8 + 8 + 1 = 153 bytes

// ─── ZK Compression version (optional) ─────────────────────────────────
#[cfg(feature = "zk-compression")]
use light_sdk::LightDiscriminator;

/// Compressed merchant DID account stored via ZK Compression.
/// Uses `#[event]` (not `#[account]`) so Anchor includes it in the IDL
/// without allocating on-chain account space. The actual data lives as
/// a compressed account hash in a Light Protocol state tree.
#[cfg(feature = "zk-compression")]
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
