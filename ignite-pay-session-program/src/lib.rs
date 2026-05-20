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

#![allow(unexpected_cfgs)]
#![allow(deprecated)]
pub mod state;
pub mod error;

use anchor_lang::prelude::*;
use anchor_spl::token;
use crate::state::SessionKeyAccount;
use crate::error::SessionError;

declare_id!("Avu35SYnvcSpWeYQhC7w2XT6DCurhnYB5PdajTqet9o");

#[program]
pub mod ignite_pay_session_program {
    use super::*;

    // ─── Register Session Key ───

    #[derive(Accounts)]
    #[instruction(
        target_program: Pubkey,
        expires_at: i64,
        spending_limit: u64,
        scopes: Vec<String>,
        token_mint: Pubkey,
        per_tx_limit: u64,
        daily_tx_count_limit: u32,
    )]
    pub struct RegisterSessionKey<'info> {
        #[account(
            init,
            payer = owner,
            space = SessionKeyAccount::default_space(),
            seeds = [b"session", owner.key().as_ref(), ephemeral_signer.key().as_ref()],
            bump,
        )]
        pub session: Account<'info, SessionKeyAccount>,
        #[account(mut)]
        pub owner: Signer<'info>,
        pub ephemeral_signer: Signer<'info>,
        /// CHECK: Target program validated off-chain
        pub target_program: UncheckedAccount<'info>,
        pub system_program: Program<'info, System>,
        pub clock: Sysvar<'info, Clock>,
    }

    pub fn register_session_key(
        ctx: Context<RegisterSessionKey>,
        target_program: Pubkey,
        expires_at: i64,
        spending_limit: u64,
        scopes: Vec<String>,
        token_mint: Pubkey,
        per_tx_limit: u64,
        daily_tx_count_limit: u32,
    ) -> Result<()> {
        let now = ctx.accounts.clock.unix_timestamp;
        require!(expires_at > now, SessionError::SessionExpired);
        require!(!scopes.is_empty(), SessionError::InvalidScope);
        for scope in &scopes {
            require!(scope.contains(':'), SessionError::InvalidScope);
        }
        let session = &mut ctx.accounts.session;
        session.owner = ctx.accounts.owner.key();
        session.ephemeral_signer = ctx.accounts.ephemeral_signer.key();
        session.target_program = target_program;
        session.token_mint = token_mint;
        session.expires_at = expires_at;
        session.spending_limit = spending_limit;
        session.current_spent = 0;
        session.per_tx_limit = per_tx_limit;
        session.daily_tx_count_limit = daily_tx_count_limit;
        session.current_daily_count = 0;
        session.last_daily_reset = now;
        session.scopes = scopes;
        session.revoked = false;
        session.bump = ctx.bumps.session;
        Ok(())
    }

    // ─── Execute Payment ───

    #[derive(Accounts)]
    pub struct ExecutePayment<'info> {
        #[account(
            mut,
            constraint = !session.revoked @ SessionError::SessionRevoked,
            constraint = session.ephemeral_signer == ephemeral_signer.key() @ SessionError::Unauthorized,
        )]
        pub session: Account<'info, SessionKeyAccount>,
        pub ephemeral_signer: Signer<'info>,
        /// CHECK: Recipient of the SOL transfer
        #[account(mut)]
        pub recipient: UncheckedAccount<'info>,
        pub system_program: Program<'info, System>,
        pub clock: Sysvar<'info, Clock>,
    }

    pub fn execute_payment(
        ctx: Context<ExecutePayment>,
        amount: u64,
        scope: String,
    ) -> Result<()> {
        let now = ctx.accounts.clock.unix_timestamp;

        // Validation phase (read-only)
        {
            let session = &ctx.accounts.session;
            require!(now < session.expires_at, SessionError::SessionExpired);
            require!(!session.revoked, SessionError::SessionRevoked);
            require!(session.scopes.contains(&scope), SessionError::ScopeNotPermitted);
            if session.per_tx_limit > 0 {
                require!(amount <= session.per_tx_limit, SessionError::PerTxLimitExceeded);
            }
        }

        // Daily window reset + count check (mutable but brief)
        {
            let session = &mut ctx.accounts.session;
            if now - session.last_daily_reset >= 86400 {
                session.current_daily_count = 0;
                session.last_daily_reset = now;
            }
            if session.daily_tx_count_limit > 0 {
                require!(
                    session.current_daily_count + 1 <= session.daily_tx_count_limit,
                    SessionError::DailyTxCountExceeded
                );
            }
            let new_spent = session.current_spent.checked_add(amount)
                .ok_or(SessionError::ArithmeticOverflow)?;
            require!(new_spent <= session.spending_limit, SessionError::SpendingLimitExceeded);
        }

        // Transfer SOL from PDA directly via lamport manipulation.
        // Cannot use System Program transfer because the PDA account carries data
        // (Anchor init creates it with serialized state), and System Program rejects
        // transfers from accounts with data.
        {
            let session_info = ctx.accounts.session.to_account_info();
            let recipient_info = ctx.accounts.recipient.to_account_info();
            let session_lamports = session_info.lamports();
            if session_lamports < amount {
                return Err(SessionError::InsufficientVaultBalance.into());
            }
            **session_info.lamports.borrow_mut() = session_lamports - amount;
            **recipient_info.lamports.borrow_mut() += amount;
        }

        // Update spent amount
        let session = &mut ctx.accounts.session;
        session.current_spent = session.current_spent.checked_add(amount)
            .ok_or(SessionError::ArithmeticOverflow)?;
        session.current_daily_count = session.current_daily_count.saturating_add(1);
        Ok(())
    }

    // ─── Execute SPL Payment ───

    #[derive(Accounts)]
    pub struct ExecuteSplPayment<'info> {
        #[account(
            mut,
            constraint = !session.revoked @ SessionError::SessionRevoked,
            constraint = session.ephemeral_signer == ephemeral_signer.key() @ SessionError::Unauthorized,
        )]
        pub session: Account<'info, SessionKeyAccount>,
        pub ephemeral_signer: Signer<'info>,
        /// CHECK: Source ATA — session PDA's ATA for the mint.
        #[account(mut)]
        pub source_ata: UncheckedAccount<'info>,
        /// CHECK: Destination ATA (recipient's ATA for the mint).
        #[account(mut)]
        pub dest_ata: UncheckedAccount<'info>,
        /// CHECK: Token mint — validated against session.token_mint.
        pub token_mint: AccountInfo<'info>,
        /// SPL Token program.
        pub token_program: Program<'info, token::Token>,
        pub clock: Sysvar<'info, Clock>,
    }

    pub fn execute_spl_payment(
        ctx: Context<ExecuteSplPayment>,
        amount: u64,
        scope: String,
    ) -> Result<()> {
        let now = ctx.accounts.clock.unix_timestamp;

        // Validation phase (read-only)
        {
            let session = &ctx.accounts.session;
            require!(now < session.expires_at, SessionError::SessionExpired);
            require!(!session.revoked, SessionError::SessionRevoked);
            require!(session.scopes.contains(&scope), SessionError::ScopeNotPermitted);
            require!(
                session.token_mint == ctx.accounts.token_mint.key(),
                SessionError::InvalidMint
            );
            require!(
                session.token_mint != Pubkey::default(),
                SessionError::SolSessionOnly
            );
            if session.per_tx_limit > 0 {
                require!(amount <= session.per_tx_limit, SessionError::PerTxLimitExceeded);
            }
        }

        // Daily window reset + limit check (mutable but brief)
        {
            let session = &mut ctx.accounts.session;
            if now - session.last_daily_reset >= 86400 {
                session.current_daily_count = 0;
                session.last_daily_reset = now;
            }
            if session.daily_tx_count_limit > 0 {
                require!(
                    session.current_daily_count + 1 <= session.daily_tx_count_limit,
                    SessionError::DailyTxCountExceeded
                );
            }
            let new_spent = session.current_spent.checked_add(amount)
                .ok_or(SessionError::ArithmeticOverflow)?;
            require!(new_spent <= session.spending_limit, SessionError::SpendingLimitExceeded);
        }

        // Execute SPL token transfer via CPI using PDA as authority
        let bump = ctx.accounts.session.bump;
        let owner = ctx.accounts.session.owner;
        let ephemeral = ctx.accounts.session.ephemeral_signer;
        let seeds = &[b"session", owner.as_ref(), ephemeral.as_ref(), &[bump]];
        let signer_seeds: &[&[&[u8]]] = &[seeds];

        let cpi_accounts = token::Transfer {
            from: ctx.accounts.source_ata.to_account_info(),
            to: ctx.accounts.dest_ata.to_account_info(),
            authority: ctx.accounts.session.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.key();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer_seeds);
        token::transfer(cpi_ctx, amount)?;

        // Update spent amount
        let session = &mut ctx.accounts.session;
        session.current_spent = session.current_spent.checked_add(amount)
            .ok_or(SessionError::ArithmeticOverflow)?;
        session.current_daily_count = session.current_daily_count.saturating_add(1);

        Ok(())
    }

    // ─── Withdraw Remaining SOL from PDA ───

    #[derive(Accounts)]
    pub struct WithdrawRemaining<'info> {
        #[account(
            mut,
            constraint = session.owner == owner.key() @ SessionError::Unauthorized,
        )]
        pub session: Account<'info, SessionKeyAccount>,
        #[account(mut)]
        pub owner: Signer<'info>,
        /// CHECK: Recipient of the SOL withdrawal
        #[account(mut)]
        pub recipient: SystemAccount<'info>,
        pub system_program: Program<'info, System>,
    }

    pub fn withdraw_remaining(ctx: Context<WithdrawRemaining>) -> Result<()> {
        let session_info = ctx.accounts.session.to_account_info();
        let recipient_info = ctx.accounts.recipient.to_account_info();

        let balance = session_info.lamports();
        // Leave rent-exempt minimum to keep the account alive
        let rent_exempt = Rent::get()?.minimum_balance(session_info.data_len());
        let withdraw_amount = balance.saturating_sub(rent_exempt);
        if withdraw_amount == 0 {
            return Ok(());
        }

        **session_info.lamports.borrow_mut() = balance - withdraw_amount;
        **recipient_info.lamports.borrow_mut() += withdraw_amount;
        Ok(())
    }

    // ─── Withdraw Remaining SPL Tokens from PDA ATA ───

    #[derive(Accounts)]
    pub struct WithdrawSplRemaining<'info> {
        #[account(
            mut,
            constraint = session.owner == owner.key() @ SessionError::Unauthorized,
        )]
        pub session: Account<'info, SessionKeyAccount>,
        #[account(mut)]
        pub owner: Signer<'info>,
        /// CHECK: PDA's ATA (source)
        #[account(mut)]
        pub source_ata: UncheckedAccount<'info>,
        /// CHECK: Owner's (or recipient's) ATA (destination)
        #[account(mut)]
        pub dest_ata: UncheckedAccount<'info>,
        pub token_program: Program<'info, token::Token>,
    }

    pub fn withdraw_spl_remaining(ctx: Context<WithdrawSplRemaining>, amount: u64) -> Result<()> {
        let session = &ctx.accounts.session;
        let bump = session.bump;
        let owner = session.owner;
        let ephemeral = session.ephemeral_signer;
        let seeds = &[b"session", owner.as_ref(), ephemeral.as_ref(), &[bump]];
        let signer_seeds: &[&[&[u8]]] = &[seeds];

        let cpi_accounts = token::Transfer {
            from: ctx.accounts.source_ata.to_account_info(),
            to: ctx.accounts.dest_ata.to_account_info(),
            authority: ctx.accounts.session.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.key();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer_seeds);
        token::transfer(cpi_ctx, amount)?;

        Ok(())
    }

    // ─── Revoke Session ───

    #[derive(Accounts)]
    pub struct RevokeSession<'info> {
        #[account(
            mut,
            constraint = session.owner == owner.key() @ SessionError::Unauthorized,
        )]
        pub session: Account<'info, SessionKeyAccount>,
        pub owner: Signer<'info>,
    }

    pub fn revoke_session(ctx: Context<RevokeSession>) -> Result<()> {
        let session = &mut ctx.accounts.session;
        require!(!session.revoked, SessionError::SessionRevoked);
        session.revoked = true;
        Ok(())
    }

    // ─── Close Session ───

    #[derive(Accounts)]
    pub struct CloseSession<'info> {
        #[account(
            mut,
            constraint = session.owner == owner.key() @ SessionError::Unauthorized,
            constraint = session.revoked || clock.unix_timestamp >= session.expires_at @ SessionError::SessionStillActive,
            close = owner,
        )]
        pub session: Account<'info, SessionKeyAccount>,
        #[account(mut)]
        pub owner: Signer<'info>,
        pub clock: Sysvar<'info, Clock>,
    }

    pub fn close_session(_ctx: Context<CloseSession>) -> Result<()> {
        Ok(())
    }
}
