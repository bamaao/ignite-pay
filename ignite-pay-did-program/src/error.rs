use anchor_lang::prelude::*;

#[error_code]
pub enum DidError {
    #[msg("DID config already initialized")]
    AlreadyInitialized,
    #[msg("DID config not initialized")]
    NotInitialized,
    #[msg("Invalid platform authority")]
    InvalidAuthority,
    #[msg("Invalid Ed25519 signature")]
    InvalidSignature,
    #[msg("Invalid DID format")]
    InvalidDidFormat,
    #[msg("Invalid status value")]
    InvalidStatus,
    #[msg("Invalid leaf hash")]
    InvalidLeafHash,
    #[msg("Merkle proof verification failed")]
    ProofVerificationFailed,
    #[msg("CPI to account-compression failed")]
    CpiFailed,
    #[msg("Invalid public key")]
    InvalidPubkey,
    #[msg("Invalid signer")]
    InvalidSigner,
    #[msg("Arithmetic overflow")]
    ArithmeticOverflow,
}
