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
pub enum DidError {
    #[msg("DID account already initialized")]
    AlreadyInitialized,
    #[msg("DID account not initialized")]
    NotInitialized,
    #[msg("Invalid platform authority")]
    InvalidAuthority,
    #[msg("Invalid public key")]
    InvalidPubkey,
    #[msg("Invalid signer")]
    InvalidSigner,
    #[msg("Arithmetic overflow")]
    ArithmeticOverflow,
    #[msg("VC subject does not match DID controller")]
    VcSubjectMismatch,
    #[msg("DID already initialized for this original key")]
    DidAlreadyInitialized,
    #[msg("Invalid controller key")]
    InvalidControllerKey,
    #[msg("Nonce mismatch")]
    NonceMismatch,
    #[msg("Invalid recovery key")]
    InvalidRecoveryKey,
    #[msg("Invalid address tree")]
    InvalidAddressTree,
    #[msg("Insufficient accounts for CPI")]
    InsufficientCpiAccounts,
    #[msg("Platform config not initialized")]
    PlatformNotInitialized,
    #[msg("Invalid platform signature")]
    InvalidPlatformSignature,
    #[msg("VC already revoked")]
    AlreadyRevoked,
    #[msg("Unauthorized to revoke VC")]
    UnauthorizedRevocation,
}
