use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use crate::state::{ChannelAccount, ChannelStatus};
use crate::error::ChannelError;

#[derive(Accounts)]
pub struct FundChannel<'info> {
    #[account(
        mut,
        constraint = channel.status == ChannelStatus::Open @ ChannelError::InvalidStatus,
        constraint = channel.deposit_b == 0 @ ChannelError::AlreadyFunded,
    )]
    pub channel: Account<'info, ChannelAccount>,

    /// CHECK: Must match channel.provider_pubkey
    #[account(
        constraint = signer.key() == channel.provider_pubkey @ ChannelError::Unauthorized,
    )]
    pub signer: Signer<'info>,

    /// Provider's source token account.
    #[account(mut)]
    pub source_vault: Account<'info, TokenAccount>,

    /// Channel's provider vault (destination).
    #[account(
        mut,
        constraint = vault_b.key() == channel.vault_b @ ChannelError::InvalidOwner,
    )]
    pub vault_b: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn fund_channel(ctx: Context<FundChannel>, deposit_b: u64) -> Result<()> {
    require!(deposit_b > 0, ChannelError::ZeroDeposit);

    // Transfer tokens from provider's source to channel's provider vault
    let cpi_accounts = Transfer {
        from: ctx.accounts.source_vault.to_account_info(),
        to: ctx.accounts.vault_b.to_account_info(),
        authority: ctx.accounts.signer.to_account_info(),
    };
    let cpi_program = ctx.accounts.token_program.to_account_info();
    token::transfer(CpiContext::new(cpi_program, cpi_accounts), deposit_b)?;

    // Update channel state
    let channel = &mut ctx.accounts.channel;
    channel.deposit_b = deposit_b;
    channel.total_deposited = channel.total_deposited
        .checked_add(deposit_b)
        .ok_or(ChannelError::ArithmeticOverflow)?;
    // PROG-6 fix: track new provider leaf
    channel.leaf_count = channel.leaf_count
        .checked_add(1)
        .ok_or(ChannelError::ArithmeticOverflow)?;

    Ok(())
}
