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
use crate::state::{ChannelAccount, ChannelStatus};
use crate::error::ChannelError;
use crate::utils::ed25519::verify_ed25519_signature;

#[derive(Accounts)]
pub struct CooperativeSettle<'info> {
    #[account(
        mut,
        constraint = channel.status == ChannelStatus::Open @ ChannelError::InvalidStatus,
    )]
    pub channel: Account<'info, ChannelAccount>,

    pub clock: Sysvar<'info, Clock>,
}

/// Cooperative close: both parties have signed the current root off-chain.
///
/// Validation:
/// - Channel must be Open
/// - sig_a and sig_b are verified on-chain using Ed25519 verification
///
/// After verification, transitions to Settling and sets settle_deadline.
pub fn cooperative_settle(
    ctx: Context<CooperativeSettle>,
    sequence: u64,
    root: [u8; 32],
    settle_window: u64,
    sig_a: [u8; 64],
    sig_b: [u8; 64],
) -> Result<()> {
    let channel = &mut ctx.accounts.channel;
    let current_slot = ctx.accounts.clock.slot;

    // BUG-37 fix: verify the submitted sequence is at least the channel's sequence.
    // Using >= allows cooperative settle with the latest or any newer signed state.
    require!(
        sequence >= channel.sequence,
        ChannelError::InvalidSequence
    );
    require!(
        root == channel.current_root,
        ChannelError::PrevHashMismatch
    );

    // PROG-8/11 fix: verify dual signatures on the cooperative settle message
    // Message: channel_id || sequence (LE) || root
    let mut msg = Vec::with_capacity(32 + 8 + 32);
    msg.extend_from_slice(&channel.channel_id);
    msg.extend_from_slice(&sequence.to_le_bytes());
    msg.extend_from_slice(&root);

    require!(
        verify_ed25519_signature(&msg, &sig_a, &channel.user_pubkey),
        ChannelError::InvalidSignature
    );
    require!(
        verify_ed25519_signature(&msg, &sig_b, &channel.provider_pubkey),
        ChannelError::InvalidSignature
    );

    channel.status = ChannelStatus::Settling;
    channel.settle_deadline = Some(current_slot + settle_window);

    Ok(())
}
