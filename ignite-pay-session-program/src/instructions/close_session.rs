use anchor_lang::prelude::*;
use crate::state::SessionKeyAccount;
use crate::error::SessionError;

#[derive(Accounts)]
pub struct CloseSession<'info> {
    #[account(
        mut,
        constraint = session.owner == owner.key() @ SessionError::Unauthorized,
        constraint = session.revoked || clock.unix_timestamp >= session.expires_at @ SessionError::SessionStillActive,
        close = owner,
    )]
    pub session: Account<'info, SessionKeyAccount>,

    /// Owner of the session — must sign and receives the rent refund.
    #[account(mut)]
    pub owner: Signer<'info>,

    pub clock: Sysvar<'info, Clock>,
}

pub fn close_session(_ctx: Context<CloseSession>) -> Result<()> {
    Ok(())
}
