use thiserror::Error;

#[derive(Error, Debug)]
pub enum SolanaError {
    #[error("RPC error: {0}")]
    RpcError(String),

    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    #[error("Invalid keypair: {0}")]
    InvalidKeypair(String),

    #[error("Invalid pubkey: {0}")]
    InvalidPubkey(String),

    #[error("Session expired")]
    SessionExpired,

    #[error("Spending limit exceeded: spent {current}, limit {limit}")]
    SpendingLimitExceeded { current: u64, limit: u64 },

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Compression error: {0}")]
    CompressionError(String),

    #[error("Proof error: {0}")]
    ProofError(String),

    #[error("Merchant not found: {0}")]
    MerchantNotFound(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Borsh error: {0}")]
    BorshError(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Sled error: {0}")]
    SledError(#[from] sled::Error),

    #[error("BS58 decode error: {0}")]
    Bs58Error(#[from] bs58::decode::Error),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, SolanaError>;
