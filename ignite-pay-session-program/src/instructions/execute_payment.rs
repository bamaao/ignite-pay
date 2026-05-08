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
use anchor_lang::solana_program::system_instruction;
use crate::state::SessionKeyAccount;
use crate::error::SessionError;

#[derive(Accounts)]
pub struct ExecutePayment<'info> {
    #[account(
        mut,
        constraint = !session.revoked @ SessionError::SessionRevoked,
        constraint = session.ephemeral_signer == ephemeral_signer.key() @ SessionError::Unauthorized,
    )]
    pub session: Account<'info, SessionKeyAccount>,

    /// The ephemeral signer — must sign and is the source of funds (self-funded mode).
    #[account(mut)]
    pub ephemeral_signer: Signer<'info>,

    /// CHECK: Recipient of the SOL transfer.
    #[account(mut)]
    pub recipient: AccountInfo<'info>,

    pub system_program: Program<'info, System>,
    pub clock: Sysvar<'info, Clock>,
}

pub fn execute_payment(
    ctx: Context<ExecutePayment>,
    amount: u64,
    scope: String,
) -> Result<()> {
    let session = &mut ctx.accounts.session;
    let clock = &ctx.accounts.clock;
    let now = clock.unix_timestamp;

    // Validate session not expired
    require!(now < session.expires_at, SessionError::SessionExpired);

    // Validate not revoked (also checked in account constraints, but defensive)
    require!(!session.revoked, SessionError::SessionRevoked);

    // Validate scope is permitted
    require!(
        session.scopes.contains(&scope),
        SessionError::ScopeNotPermitted
    );

    // Validate spending limit
    let new_spent = session
        .current_spent
        .checked_add(amount)
        .ok_or(SessionError::ArithmeticOverflow)?;
    require!(
        new_spent <= session.spending_limit,
        SessionError::SpendingLimitExceeded
    );

    // Execute SOL transfer from ephemeral signer to recipient
    let ix = system_instruction::transfer(
        &ctx.accounts.ephemeral_signer.key(),
        &ctx.accounts.recipient.key(),
        amount,
    );

    anchor_lang::solana_program::program::invoke(
        &ix,
        &[
            ctx.accounts.ephemeral_signer.to_account_info(),
            ctx.accounts.recipient.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
        ],
    )?;

    // Update spent amount
    session.current_spent = new_spent;

    Ok(())
}
