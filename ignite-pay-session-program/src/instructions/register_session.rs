use anchor_lang::prelude::*;
use crate::state::SessionKeyAccount;
use crate::error::SessionError;

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

    /// Owner of the session key — must sign and fund the PDA.
    #[account(mut)]
    pub owner: Signer<'info>,

    /// The ephemeral signer — must also sign to prove possession of the private key.
    pub ephemeral_signer: Signer<'info>,

    /// CHECK: Target program validated off-chain; on-chain we just store it.
    pub target_program: AccountInfo<'info>,

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
    let clock = &ctx.accounts.clock;
    let now = clock.unix_timestamp;

    // Validate expiry is in the future
    require!(expires_at > now, SessionError::SessionExpired);

    // Validate scopes are non-empty
    require!(!scopes.is_empty(), SessionError::InvalidScope);

    // Validate each scope format (must contain a colon separator)
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
