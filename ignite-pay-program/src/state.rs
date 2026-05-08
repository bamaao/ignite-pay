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

/// On-chain status of a payment channel.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Debug, Copy)]
pub enum ChannelStatus {
    /// Channel is open for off-chain operations.
    Open,
    /// Channel is under dispute (challenge period active).
    Challenged,
    /// Channel is settling (claim window active).
    Settling,
    /// Channel is fully closed.
    Closed,
}

/// On-chain account storing channel state.
///
/// Mirrors the off-chain ChannelMetadata for settlement verification.
/// Stored as a PDA derived from ["channel", channel_id].
#[account]
pub struct ChannelAccount {
    /// Unique channel identifier (32 bytes).
    pub channel_id: [u8; 32],
    /// User's public key (party A).
    pub user_pubkey: Pubkey,
    /// Provider's public key (party B).
    pub provider_pubkey: Pubkey,
    /// SPL token mint for this channel.
    pub token_mint: Pubkey,
    /// Current channel status.
    pub status: ChannelStatus,
    /// Current sequence number.
    pub sequence: u64,
    /// Current Merkle root (32 bytes).
    pub current_root: [u8; 32],
    /// Total amount deposited into this channel.
    pub total_deposited: u64,
    /// Slot when the channel was opened.
    pub open_slot: u64,
    /// Slot when challenge was triggered (if any).
    pub challenge_slot: Option<u64>,
    /// User's vault (SPL token account).
    pub vault_a: Pubkey,
    /// Provider's vault (SPL token account).
    pub vault_b: Pubkey,
    /// Amount deposited by the user.
    pub deposit_a: u64,
    /// Amount deposited by the provider (dual-funded channels).
    pub deposit_b: u64,
    /// Challenge period duration in slots.
    pub challenge_duration: u64,
    /// Minimum delay before challenge can be triggered.
    pub min_challenge_delay: u64,
    /// Total amount already claimed during settlement.
    pub total_claimed: u64,
    /// Settlement window deadline slot.
    pub settle_deadline: Option<u64>,
    /// Tree depth for the Merkle tree.
    pub tree_depth: u32,
    /// Number of non-empty leaves in the tree.
    pub leaf_count: u32,
    /// Set of leaf indices that have already been claimed (duplicate claim prevention).
    pub claimed_leaves: Vec<u32>,
    /// Slot at which the channel auto-closes if no activity (§3.4.3).
    pub auto_close_slot: Option<u64>,
}

impl ChannelAccount {
    /// Calculate the space required for a ChannelAccount.
    ///
    /// PROG-9 fix: use exact Anchor serialization sizes:
    /// - enum: 1 byte (no padding needed for Copy enums)
    /// - Option<u64>: 1 + 8 = 9 bytes
    /// - Vec<u32>: 4 (length prefix) + max_entries * 4
    /// - auto_close_slot: 1 + 8 (Option<u64>)
    ///
    /// Max claimed_leaves bounded by tree_depth: 2^tree_depth entries.
    /// With tree_depth up to 12, max = 4096 entries.
    pub fn space(tree_depth: u32) -> usize {
        let max_leaves = 1u32 << tree_depth.min(12); // cap at 4096
        8 + // discriminator
        32 + // channel_id
        32 + // user_pubkey
        32 + // provider_pubkey
        32 + // token_mint
        1 + // status (Anchor enum = 1 byte)
        8 + // sequence
        32 + // current_root
        8 + // total_deposited
        8 + // open_slot
        1 + 8 + // challenge_slot (Option<u64>)
        32 + // vault_a
        32 + // vault_b
        8 + // deposit_a
        8 + // deposit_b
        8 + // challenge_duration
        8 + // min_challenge_delay
        8 + // total_claimed
        1 + 8 + // settle_deadline (Option<u64>)
        4 + // tree_depth
        4 + // leaf_count
        4 + (max_leaves as usize) * 4 + // claimed_leaves
        1 + 8 // auto_close_slot (Option<u64>)
    }
}
