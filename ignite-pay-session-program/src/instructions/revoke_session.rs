use anchor_lang::prelude::*;
use crate::state::SessionKeyAccount;
use crate::error::SessionError;

#[derive(Accounts)]
pub struct RevokeSession<'info> {
    #[account(
        mut,
        constraint = session.owner == owner.key() @ SessionError::Unauthorized,
    )]
    pub session: Account<'info, SessionKeyAccount>,

    /// Owner of the session — must sign to revoke.
    pub owner: Signer<'info>,
}

pub fn revoke_session(_ctx: Context<RevokeSession>) -> Result<()> {
    let session = &mut _ctx.accounts.session;
    require!(!session.revoked, SessionError::SessionRevoked);
    session.revoked = true;
    Ok(())
}
