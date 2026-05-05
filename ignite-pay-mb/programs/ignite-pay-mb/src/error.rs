use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    #[msg("Exceeds the allowed spending cap.")]
    SpendingCapExceeded,
    #[msg("Insufficient balance in the channel.")]
    InsufficientBalance,
    #[msg("Settlement already claimed.")]
    AlreadyClaimed,
    #[msg("Settlement is under dispute.")]
    Disputed,
    #[msg("Settlement already disputed.")]
    AlreadyDisputed,
    #[msg("Challenge period has not expired.")]
    ChallengePeriodNotExpired,
    #[msg("Challenge period has expired.")]
    ChallengePeriodExpired,
    #[msg("Only the buyer can dispute.")]
    NotBuyer,
    #[msg("Arithmetic overflow.")]
    ArithmeticOverflow,
    #[msg("Invalid Ed25519 signature instruction.")]
    InvalidSignatureInstruction,
    #[msg("Signature does not match expected value.")]
    SignatureMismatch,
    #[msg("Invalid transaction layout: at least one Ed25519 instruction must precede settle_batch.")]
    InvalidTransactionLayout,
    #[msg("Settlement is not disputed.")]
    NotDisputed,
    #[msg("Dispute period has not expired.")]
    DisputePeriodNotExpired,
    #[msg("Invalid fraud proof.")]
    InvalidFraudProof,
    #[msg("Fraud not proven: actual total matches claimed total.")]
    FraudNotProven,
    #[msg("Buyer Ed25519 signature instruction not found in transaction.")]
    BuyerSignatureNotFound,
    #[msg("Merchant Ed25519 signature instruction not found in transaction.")]
    MerchantSignatureNotFound,
    #[msg("Total allocated spending caps exceed total deposited funds.")]
    AllocationExceedsDeposit,
    #[msg("Optimistic settlement requires challenge_period > 0.")]
    ChallengePeriodRequired,
    #[msg("Token mint mismatch.")]
    TokenMintMismatch,
}
