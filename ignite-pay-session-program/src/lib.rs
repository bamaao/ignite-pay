pub mod state;
pub mod error;

use anchor_lang::prelude::*;
use anchor_spl::token;
use anchor_spl::token::Token;
use crate::state::SessionKeyAccount;
use crate::error::SessionError;

declare_id!("6EFvVTh7rEBpHH2JGryjKQmBLRtbYtSEerGNfkHqKiei");

#[program]
pub mod ignite_pay_session_program {
    use super::*;
    use anchor_lang::solana_program::system_instruction;

    // ─── Register Session Key ───

    #[derive(Accounts)]
    #[instruction(
        target_program: Pubkey,
        expires_at: i64,
        spending_limit: u64,
        scopes: Vec<String>,
        token_mint: Pubkey,
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
        #[account(mut)]
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
        let session = &mut ctx.accounts.session;
        let now = ctx.accounts.clock.unix_timestamp;
        require!(now < session.expires_at, SessionError::SessionExpired);
        require!(!session.revoked, SessionError::SessionRevoked);
        require!(session.scopes.contains(&scope), SessionError::ScopeNotPermitted);
        let new_spent = session.current_spent.checked_add(amount)
            .ok_or(SessionError::ArithmeticOverflow)?;
        require!(new_spent <= session.spending_limit, SessionError::SpendingLimitExceeded);

        let ix = system_instruction::transfer(
            ctx.accounts.ephemeral_signer.key,
            ctx.accounts.recipient.key,
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
        session.current_spent = new_spent;
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
        #[account(mut)]
        pub ephemeral_signer: Signer<'info>,
        /// CHECK: Source Associated Token Account (ephemeral's ATA for the mint).
        #[account(mut)]
        pub source_ata: UncheckedAccount<'info>,
        /// CHECK: Destination Associated Token Account (recipient's ATA for the mint).
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
        let session = &mut ctx.accounts.session;
        let now = ctx.accounts.clock.unix_timestamp;

        // Validate session not expired
        require!(now < session.expires_at, SessionError::SessionExpired);

        // Validate not revoked
        require!(!session.revoked, SessionError::SessionRevoked);

        // Validate scope is permitted
        require!(
            session.scopes.contains(&scope),
            SessionError::ScopeNotPermitted
        );

        // Validate mint matches session
        require!(
            session.token_mint == ctx.accounts.token_mint.key(),
            SessionError::InvalidMint
        );

        // Validate this is not a SOL-only session
        require!(
            session.token_mint != Pubkey::default(),
            SessionError::SolSessionOnly
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

        // Execute SPL token transfer via CPI using anchor_spl
        let cpi_accounts = token::Transfer {
            from: ctx.accounts.source_ata.to_account_info(),
            to: ctx.accounts.dest_ata.to_account_info(),
            authority: ctx.accounts.ephemeral_signer.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.key();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, amount)?;

        // Update spent amount
        session.current_spent = new_spent;

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
