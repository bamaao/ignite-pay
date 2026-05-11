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

#[error_code]
pub enum SessionError {
    #[msg("Session has been revoked")]
    SessionRevoked,

    #[msg("Session has expired")]
    SessionExpired,

    #[msg("Spending limit exceeded")]
    SpendingLimitExceeded,

    #[msg("Scope not permitted for this session")]
    ScopeNotPermitted,

    #[msg("Unauthorized: signer is not the session owner")]
    Unauthorized,

    #[msg("Session is still active (not expired or revoked)")]
    SessionStillActive,

    #[msg("Arithmetic overflow")]
    ArithmeticOverflow,

    #[msg("Invalid scope format")]
    InvalidScope,

    #[msg("Invalid mint: session token_mint mismatch")]
    InvalidMint,

    #[msg("Session is for SOL, not SPL tokens")]
    SolSessionOnly,

    #[msg("Per-transaction spending limit exceeded")]
    PerTxLimitExceeded,

    #[msg("Daily transaction count limit exceeded")]
    DailyTxCountExceeded,
}
