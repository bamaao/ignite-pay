use anchor_lang::prelude::*;
use crate::state::{ChannelAccount, ChannelStatus};
use crate::error::ChannelError;
use crate::utils::ed25519::verify_ed25519_signature;

#[derive(Accounts)]
pub struct SubmitCounterState<'info> {
    #[account(
        mut,
        constraint = channel.status == ChannelStatus::Challenged @ ChannelError::InvalidStatus,
    )]
    pub channel: Account<'info, ChannelAccount>,
}

/// Submit a counter-state during the challenge period.
///
/// The counter-party submits a higher-sequence dual-signed state.
/// BUG-33 fix: signatures are now verified on-chain using Ed25519 verification.
pub fn submit_counter_state(
    ctx: Context<SubmitCounterState>,
    sequence: u64,
    root: [u8; 32],
    sig_a: [u8; 64],
    sig_b: [u8; 64],
) -> Result<()> {
    let channel = &mut ctx.accounts.channel;

    // Counter state must have higher sequence
    require!(
        sequence > channel.sequence,
        ChannelError::InvalidSequence
    );

    // BUG-33 fix: verify both signatures on-chain
    // Message = channel_id || sequence || root
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

    channel.sequence = sequence;
    channel.current_root = root;

    Ok(())
}
