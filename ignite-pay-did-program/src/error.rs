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
}
