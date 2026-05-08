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

use crate::error::ErrorCode;

// Ed25519 precompiled program ID: Ed25519SigVerify111111111111111111111111111
const ED25519_PROGRAM_ID: [u8; 32] = [
    3, 125, 70, 214, 124, 147, 251, 190, 18, 249, 66, 143, 131, 141, 64, 255,
    5, 112, 116, 73, 39, 244, 138, 100, 252, 202, 112, 68, 128, 0, 0, 0,
];

pub enum VerifiedParty {
    Buyer,
    Merchant,
}

/// Verify Ed25519 signatures using the solana_instructions_sysvar crate
/// to properly load and parse instruction data.
pub fn verify_ed25519_for_pubkey(
    ix_sysvar: &AccountInfo<'_>,
    ix_index: usize,
    expected_message: &[u8],
    buyer_sig: &[u8],
    merchant_sig: &[u8],
    buyer_pubkey: &[u8],
    merchant_pubkey: &[u8],
) -> Result<VerifiedParty> {
    let current_ix = solana_instructions_sysvar::load_current_index_checked(ix_sysvar)
        .map_err(|_| ErrorCode::InvalidSignatureInstruction)? as usize;

    if ix_index >= current_ix {
        return Err(ErrorCode::InvalidSignatureInstruction.into());
    }

    let instruction = solana_instructions_sysvar::load_instruction_at_checked(ix_index, ix_sysvar)
        .map_err(|_| ErrorCode::InvalidSignatureInstruction)?;

    if instruction.program_id.as_ref() != ED25519_PROGRAM_ID {
        return Err(ErrorCode::InvalidSignatureInstruction.into());
    }

    let data = &instruction.data;
    if data.len() < 16 { return Err(ErrorCode::InvalidSignatureInstruction.into()); }

    let num_sigs = data[0] as usize;
    if num_sigs == 0 { return Err(ErrorCode::InvalidSignatureInstruction.into()); }

    for sig_idx in 0..num_sigs {
        let header_offset = 2 + sig_idx * 14;
        if data.len() < header_offset + 14 { continue; }

        let sig_offset = u16::from_le_bytes([data[header_offset], data[header_offset + 1]]) as usize;
        let pubkey_offset = u16::from_le_bytes([data[header_offset + 4], data[header_offset + 5]]) as usize;
        let msg_offset = u16::from_le_bytes([data[header_offset + 8], data[header_offset + 9]]) as usize;
        let msg_size = u16::from_le_bytes([data[header_offset + 10], data[header_offset + 11]]) as usize;

        if data.len() < pubkey_offset + 32 { continue; }
        let pk = &data[pubkey_offset..pubkey_offset + 32];

        let (expected_sig, party) = if pk == buyer_pubkey {
            (buyer_sig, VerifiedParty::Buyer)
        } else if pk == merchant_pubkey {
            (merchant_sig, VerifiedParty::Merchant)
        } else {
            continue;
        };

        if data.len() < sig_offset + 64 { continue; }
        if &data[sig_offset..sig_offset + 64] != expected_sig { continue; }

        if data.len() < msg_offset + msg_size { continue; }
        if &data[msg_offset..msg_offset + msg_size] != expected_message { continue; }

        return Ok(party);
    }

    Err(ErrorCode::InvalidSignatureInstruction.into())
}
