use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use crate::state::{ChannelAccount, ChannelStatus};
use crate::error::ChannelError;
use crate::utils::ed25519::verify_ed25519_signature;

#[derive(Accounts)]
pub struct FinalizeSettlement<'info> {
    #[account(
        mut,
        constraint = channel.status == ChannelStatus::Settling @ ChannelError::InvalidStatus,
    )]
    pub channel: Account<'info, ChannelAccount>,

    /// CHECK: Must be a channel participant
    #[account(
        constraint = caller.key() == channel.user_pubkey
            || caller.key() == channel.provider_pubkey
            @ ChannelError::Unauthorized,
    )]
    pub caller: Signer<'info>,

    /// User's vault for refund.
    #[account(mut)]
    pub vault_a: Account<'info, TokenAccount>,

    /// Provider's vault for refund.
    #[account(mut)]
    pub vault_b: Account<'info, TokenAccount>,

    /// Channel's escrow vault holding deposited tokens.
    /// CHECK: PDA escrow vault - verified by seeds
    #[account(
        mut,
        seeds = [b"escrow", channel.channel_id.as_ref()],
        bump,
    )]
    pub escrow_vault: AccountInfo<'info>,

    pub token_program: Program<'info, Token>,
    pub clock: Sysvar<'info, Clock>,
}

/// Finalize settlement after the settle_deadline has passed.
///
/// Computes proportional refunds for unclaimed funds and transfers tokens.
/// Transitions channel to Closed.
pub fn finalize_settlement(
    ctx: Context<FinalizeSettlement>,
    caller_signature: [u8; 64],
) -> Result<()> {
    let channel = &mut ctx.accounts.channel;
    let current_slot = ctx.accounts.clock.slot;

    let deadline = channel.settle_deadline
        .ok_or(ChannelError::InvalidStatus)?;
    require!(
        current_slot >= deadline,
        ChannelError::SettlementNotExpired
    );

    // PROG-8 fix: verify caller's signature
    let mut fin_msg = Vec::with_capacity(32 + 8 + 32);
    fin_msg.extend_from_slice(&channel.channel_id);
    fin_msg.extend_from_slice(&current_slot.to_le_bytes());
    fin_msg.extend_from_slice(&channel.current_root);
    require!(
        verify_ed25519_signature(&fin_msg, &caller_signature, &ctx.accounts.caller.key()),
        ChannelError::InvalidSignature
    );

    // Compute proportional refunds for unclaimed funds
    let unclaimed = channel.total_deposited
        .checked_sub(channel.total_claimed)
        .ok_or(ChannelError::ArithmeticOverflow)?;

    if unclaimed > 0 {
        let total_deposit = channel.deposit_a
            .checked_add(channel.deposit_b)
            .ok_or(ChannelError::ArithmeticOverflow)?;

        if total_deposit > 0 {
            // Proportional split based on original deposits
            let ratio_a = (channel.deposit_a as u128)
                .checked_mul(1_000_000)
                .ok_or(ChannelError::ArithmeticOverflow)?
                .checked_div(total_deposit as u128)
                .ok_or(ChannelError::ArithmeticOverflow)?;

            let refund_a = (unclaimed as u128)
                .checked_mul(ratio_a)
                .ok_or(ChannelError::ArithmeticOverflow)?
                .checked_div(1_000_000)
                .ok_or(ChannelError::ArithmeticOverflow)? as u64;

            let refund_b = unclaimed.checked_sub(refund_a)
                .ok_or(ChannelError::ArithmeticOverflow)?;

            // PROG-7 / BUG-38 fix: Transfer refunds from escrow to respective vaults
            // using PDA signer seeds for proper authority verification
            let bump = ctx.bumps.escrow_vault;
            let seeds: &[&[u8]] = &[b"escrow", channel.channel_id.as_ref(), &[bump]];

            if refund_a > 0 {
                let cpi_accounts_a = Transfer {
                    from: ctx.accounts.escrow_vault.to_account_info(),
                    to: ctx.accounts.vault_a.to_account_info(),
                    authority: ctx.accounts.escrow_vault.to_account_info(),
                };
                token::transfer(
                    CpiContext::new_with_signer(
                        ctx.accounts.token_program.to_account_info(),
                        cpi_accounts_a,
                        &[seeds],
                    ),
                    refund_a,
                )?;
            }

            if refund_b > 0 {
                let cpi_accounts_b = Transfer {
                    from: ctx.accounts.escrow_vault.to_account_info(),
                    to: ctx.accounts.vault_b.to_account_info(),
                    authority: ctx.accounts.escrow_vault.to_account_info(),
                };
                token::transfer(
                    CpiContext::new_with_signer(
                        ctx.accounts.token_program.to_account_info(),
                        cpi_accounts_b,
                        &[seeds],
                    ),
                    refund_b,
                )?;
            }
        }
    }

    channel.status = ChannelStatus::Closed;

    Ok(())
}
