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

#[derive(Accounts)]
pub struct SettleAfterTimeout<'info> {
    #[account(
        mut,
        constraint = channel.status == ChannelStatus::Challenged
            || (channel.status == ChannelStatus::Open
                && channel.auto_close_slot.is_some())
            @ ChannelError::InvalidStatus,
    )]
    pub channel: Account<'info, ChannelAccount>,

    pub clock: Sysvar<'info, Clock>,
}

/// Settle after challenge timeout or auto-close.
///
/// Transitions Challenged -> Settling after challenge_duration has elapsed.
/// BUG-41 / PROG-15 fix: also handles auto_close_slot - when an Open channel
/// has auto_close_slot set and the current slot has reached it, allows direct
/// transition to Settling without requiring a challenge first.
pub fn settle_after_timeout(
    ctx: Context<SettleAfterTimeout>,
    settle_window: u64,
) -> Result<()> {
    let channel = &mut ctx.accounts.channel;
    let current_slot = ctx.accounts.clock.slot;

    if channel.status == ChannelStatus::Open {
        // BUG-41 / PROG-15 fix: auto_close_slot path
        // Channel is Open but auto_close_slot has been reached
        let auto_close = channel.auto_close_slot
            .ok_or(ChannelError::InvalidStatus)?;
        require!(
            current_slot >= auto_close,
            ChannelError::ChallengeNotElapsed
        );
    } else {
        // Standard challenge timeout path
        let challenge_slot = channel.challenge_slot
            .ok_or(ChannelError::InvalidStatus)?;

        // BUG-19 fix: strict > per design doc §4.2 (off-chain uses strict > as well)
        require!(
            current_slot > challenge_slot + channel.challenge_duration,
            ChannelError::ChallengeNotElapsed
        );
    }

    channel.status = ChannelStatus::Settling;
    channel.settle_deadline = Some(current_slot + settle_window);

    Ok(())
}
