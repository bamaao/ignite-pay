use anchor_lang::prelude::*;
use crate::state::{ChannelAccount, ChannelStatus};
use crate::error::ChannelError;
use crate::utils::ed25519::verify_ed25519_signature;

#[derive(Accounts)]
pub struct TriggerChallenge<'info> {
    #[account(
        mut,
        constraint = channel.status == ChannelStatus::Open @ ChannelError::InvalidStatus,
    )]
    pub channel: Account<'info, ChannelAccount>,

    /// CHECK: Must be either user_pubkey or provider_pubkey
    #[account(
        constraint = challenger.key() == channel.user_pubkey
            || challenger.key() == channel.provider_pubkey
            @ ChannelError::Unauthorized,
    )]
    pub challenger: Signer<'info>,

    pub clock: Sysvar<'info, Clock>,
}

/// Trigger a dispute challenge on the channel.
///
/// Per design doc §4.2: either party submits (root, sequence, sig) and the program
/// verifies sequence > on-chain sequence. Checks min_challenge_delay (anti front-running).
/// PROG-10 fix: now verifies the challenger's Ed25519 signature.
pub fn trigger_challenge(
    ctx: Context<TriggerChallenge>,
    submitted_root: [u8; 32],
    submitted_sequence: u64,
    challenger_signature: [u8; 64],
) -> Result<()> {
    let channel = &mut ctx.accounts.channel;
    let current_slot = ctx.accounts.clock.slot;

    // Check min_challenge_delay (anti front-running)
    let min_slot = channel.open_slot + channel.min_challenge_delay;
    require!(
        current_slot >= min_slot,
        ChannelError::ChallengeNotElapsed
    );

    // Verify submitted sequence > on-chain sequence (design doc §4.2)
    require!(
        submitted_sequence > channel.sequence,
        ChannelError::InvalidSequence
    );

    // PROG-10 fix: verify challenger's signature on (channel_id, current_slot, submitted_root)
    let mut msg = Vec::with_capacity(32 + 8 + 32);
    msg.extend_from_slice(&channel.channel_id);
    msg.extend_from_slice(&current_slot.to_le_bytes());
    msg.extend_from_slice(&submitted_root);

    require!(
        verify_ed25519_signature(&msg, &challenger_signature, &ctx.accounts.challenger.key()),
        ChannelError::InvalidSignature
    );

    channel.status = ChannelStatus::Challenged;
    channel.challenge_slot = Some(current_slot);
    // Update to the submitted state
    channel.current_root = submitted_root;
    channel.sequence = submitted_sequence;

    Ok(())
}
