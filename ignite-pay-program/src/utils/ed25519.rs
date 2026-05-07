use anchor_lang::prelude::*;

/// Verify an Ed25519 signature via instruction introspection.
///
/// Finds a preceding `ed25519_program` instruction in the transaction
/// that matches the expected (pubkey, message, signature) tuple.
/// The Solana runtime performs the actual signature verification natively.
///
/// Returns the index of the matching ed25519 instruction, or error.
///
/// Note: This function parses the raw instructions sysvar data directly,
/// avoiding type mismatches between Anchor v1.0 and solana_program v2.2.
///
/// Instructions sysvar data layout (NOT bincode):
///   [0..2]   num_instructions (u16 LE)
///   [2..2+2*N] offset_table: N x u16 LE byte offsets to each instruction
///   Each instruction at its offset:
///     u16 LE num_accounts
///     per account: u8 flags + [u8;32] pubkey
///     [u8;32] program_id
///     u16 LE data_len
///     [u8; data_len] instruction data
///   Last 2 bytes: u16 LE current_index
pub fn get_ed25519_signature_verification_ix_index(
    instruction_sysvar: &AccountInfo<'_>,
    expected_pubkey: &Pubkey,
    message: &[u8],
    signature: &[u8; 64],
    current_ix_index: u8,
) -> Result<u8> {
    let ix_data = instruction_sysvar.try_borrow_data()
        .map_err(|_| crate::error::ChannelError::InvalidSignature)?;

    // Need at least: num_instructions(2) + current_index(2)
    if ix_data.len() < 4 {
        return Err(crate::error::ChannelError::InvalidSignature.into());
    }

    let num_instructions = u16::from_le_bytes([ix_data[0], ix_data[1]]) as usize;

    for ix_idx in 0..num_instructions {
        if ix_idx as u8 >= current_ix_index {
            break;
        }

        // Read offset from the offset table at bytes [2 + 2*ix_idx .. 2 + 2*ix_idx + 2]
        let table_pos = 2 + 2 * ix_idx;
        if table_pos + 2 > ix_data.len() - 2 {
            break;
        }
        let ix_offset = u16::from_le_bytes([ix_data[table_pos], ix_data[table_pos + 1]]) as usize;
        if ix_offset >= ix_data.len() - 2 {
            continue;
        }

        // Parse instruction at ix_offset:
        // u16 num_accounts, then per account: u8 flags + [u8;32] pubkey, then [u8;32] program_id, then u16 data_len + data
        let mut pos = ix_offset;

        // Read num_accounts (u16)
        if pos + 2 > ix_data.len() - 2 {
            continue;
        }
        let num_accounts = u16::from_le_bytes([ix_data[pos], ix_data[pos + 1]]) as usize;
        pos += 2;

        // Skip accounts: each is u8 flags + 32 bytes pubkey = 33 bytes
        let accounts_size = num_accounts * 33;
        pos += accounts_size;

        // Read program_id (32 bytes)
        if pos + 32 > ix_data.len() - 2 {
            continue;
        }
        let program_id = &ix_data[pos..pos + 32];
        pos += 32;

        // Check if this is an ed25519 program instruction
        // Ed25519SigVerify1111111111111111111111111111 (base58-decoded)
        let ed25519_program_id: [u8; 32] = [
            0xca, 0x62, 0x0c, 0x98, 0x39, 0x87, 0x09, 0x10,
            0x4c, 0x79, 0x14, 0x83, 0xce, 0x00, 0xb9, 0xc7,
            0x3b, 0x7a, 0x58, 0x93, 0x0d, 0x67, 0x5a, 0xe1,
            0x45, 0xdd, 0x6f, 0x85, 0x00, 0x00, 0x00, 0x00,
        ];
        if program_id != ed25519_program_id {
            continue;
        }

        // Read data_len (u16)
        if pos + 2 > ix_data.len() - 2 {
            continue;
        }
        let data_len = u16::from_le_bytes([ix_data[pos], ix_data[pos + 1]]) as usize;
        pos += 2;

        if pos + data_len > ix_data.len() - 2 {
            continue;
        }

        let ix_slice = &ix_data[pos..pos + data_len];

        // Check ed25519 pattern: data[0]==1 (num_signatures), data[1]==0 (padding)
        if data_len < 16 || ix_slice[0] != 1 || ix_slice[1] != 0 {
            continue;
        }

        let sig_offset = u16::from_le_bytes([ix_slice[2], ix_slice[3]]) as usize;
        let pk_offset = u16::from_le_bytes([ix_slice[6], ix_slice[7]]) as usize;
        let msg_offset = u16::from_le_bytes([ix_slice[10], ix_slice[11]]) as usize;
        let msg_size = u16::from_le_bytes([ix_slice[12], ix_slice[13]]) as usize;

        // Only support self-referencing (all instruction index fields == u16::MAX)
        let self_ref = u16::MAX;
        if u16::from_le_bytes([ix_slice[4], ix_slice[5]]) != self_ref
            || u16::from_le_bytes([ix_slice[8], ix_slice[9]]) != self_ref
            || u16::from_le_bytes([ix_slice[14], ix_slice[15]]) != self_ref
        {
            continue;
        }

        if pk_offset + 32 > data_len || sig_offset + 64 > data_len || msg_offset + msg_size > data_len {
            continue;
        }

        let pk = &ix_slice[pk_offset..pk_offset + 32];
        let sig = &ix_slice[sig_offset..sig_offset + 64];
        let msg = &ix_slice[msg_offset..msg_offset + msg_size];

        if pk == expected_pubkey.as_ref() && sig == signature && msg == message {
            return Ok(ix_idx as u8);
        }
    }
    Err(crate::error::ChannelError::InvalidSignature.into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_ed25519_ix_data_layout() {
        // Verify the expected layout of an ed25519 instruction
        let pk = [1u8; 32];
        let msg = b"test message";
        let sig = [2u8; 64];

        let data_start: u16 = 16;
        let sig_offset = data_start;
        let pk_offset = sig_offset + 64;
        let msg_offset = pk_offset + 32;
        let msg_size = msg.len() as u16;
        let self_ix_index = u16::MAX;

        let mut data = Vec::with_capacity(16 + 64 + 32 + msg.len());
        data.push(1u8);
        data.push(0u8);
        data.extend_from_slice(&sig_offset.to_le_bytes());
        data.extend_from_slice(&self_ix_index.to_le_bytes());
        data.extend_from_slice(&pk_offset.to_le_bytes());
        data.extend_from_slice(&self_ix_index.to_le_bytes());
        data.extend_from_slice(&msg_offset.to_le_bytes());
        data.extend_from_slice(&msg_size.to_le_bytes());
        data.extend_from_slice(&self_ix_index.to_le_bytes());
        data.extend_from_slice(&sig);
        data.extend_from_slice(&pk);
        data.extend_from_slice(msg);

        assert_eq!(data[0], 1);
        assert_eq!(data[1], 0);
        assert_eq!(u16::from_le_bytes([data[2], data[3]]), 16);
        assert_eq!(u16::from_le_bytes([data[6], data[7]]), 80);
        assert_eq!(u16::from_le_bytes([data[10], data[11]]), 112);
        assert_eq!(&data[16..80], &sig[..]);
        assert_eq!(&data[80..112], &pk[..]);
        assert_eq!(&data[112..112 + msg.len()], msg);
    }
}
