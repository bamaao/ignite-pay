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

use thiserror::Error;

#[derive(Error, Debug)]
pub enum StateChannelError {
    #[error("invalid sequence number: expected {expected}, got {actual}")]
    InvalidSequence { expected: u64, actual: u64 },

    #[error("previous leaf hash mismatch")]
    PrevHashMismatch,

    #[error("invalid signature")]
    InvalidSignature,

    #[error("insufficient balance: required {required}, available {available}")]
    InsufficientBalance { required: u64, available: u64 },

    #[error("leaf index out of bounds: {index}, max {max}")]
    LeafIndexOutOfBounds { index: usize, max: usize },

    #[error("merkle proof verification failed")]
    ProofVerificationFailed,

    #[error("channel not found: {0}")]
    ChannelNotFound(String),

    #[error("HTLC expired at slot {expiry}, current slot {current}")]
    HtlcExpired { expiry: u64, current: u64 },

    #[error("HTLC not yet expired: expires at slot {expiry}, current slot {current}")]
    HtlcNotExpired { expiry: u64, current: u64 },

    #[error("hash lock mismatch")]
    HashLockMismatch,

    #[error("invalid owner")]
    InvalidOwner,

    #[error("amount conservation violation: expected {expected}, got {actual}")]
    AmountConservation { expected: u64, actual: u64 },

    #[error("empty leaf cannot be used for this operation")]
    EmptyLeaf,

    #[error("leaf slot already occupied")]
    LeafSlotOccupied,

    #[error("no available leaf slots")]
    NoAvailableSlots,

    #[error("borsh serialization error: {0}")]
    BorshError(#[from] std::io::Error),

    #[error("sled database error: {0}")]
    SledError(#[from] sled::Error),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, StateChannelError>;
