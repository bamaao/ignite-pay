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
pub enum ChannelError {
    #[msg("Invalid sequence number")]
    InvalidSequence,

    #[msg("Previous leaf hash mismatch")]
    PrevHashMismatch,

    #[msg("Invalid signature")]
    InvalidSignature,

    #[msg("Insufficient balance")]
    InsufficientBalance,

    #[msg("Leaf index out of bounds")]
    LeafIndexOutOfBounds,

    #[msg("Merkle proof verification failed")]
    ProofVerificationFailed,

    #[msg("Channel not found")]
    ChannelNotFound,

    #[msg("HTLC has expired")]
    HtlcExpired,

    #[msg("HTLC has not expired")]
    HtlcNotExpired,

    #[msg("Hash lock mismatch")]
    HashLockMismatch,

    #[msg("Invalid owner")]
    InvalidOwner,

    #[msg("Amount conservation violation")]
    AmountConservation,

    #[msg("Empty leaf cannot be used for this operation")]
    EmptyLeaf,

    #[msg("Leaf slot already occupied")]
    LeafSlotOccupied,

    #[msg("No available leaf slots")]
    NoAvailableSlots,

    #[msg("Channel is not in the required status")]
    InvalidStatus,

    #[msg("Challenge duration has not elapsed")]
    ChallengeNotElapsed,

    #[msg("Settlement window has expired")]
    SettlementExpired,

    #[msg("Settlement window has not expired")]
    SettlementNotExpired,

    #[msg("Unauthorized: signer is not a channel participant")]
    Unauthorized,

    #[msg("Channel already has provider funding")]
    AlreadyFunded,

    #[msg("Deposit amount must be greater than zero")]
    ZeroDeposit,

    #[msg("Leaf has already been claimed")]
    AlreadyClaimed,

    #[msg("Claim amount does not match leaf amount")]
    AmountMismatch,

    #[msg("Invalid leaf data: deserialization or hash mismatch")]
    InvalidLeafData,

    #[msg("Arithmetic overflow")]
    ArithmeticOverflow,
}
