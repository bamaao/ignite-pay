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
