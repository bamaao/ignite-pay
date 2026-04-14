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
}
