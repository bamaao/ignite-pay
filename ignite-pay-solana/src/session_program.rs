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

use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
#[allow(deprecated)]
use solana_sdk::system_program;

/// Program ID for the session key program.
/// Replace with actual deployed program ID.
pub fn session_program_id() -> Pubkey {
    Pubkey::try_from("6EFvVTh7rEBpHH2JGryjKQmBLRtbYtSEerGNfkHqKiei").unwrap()
}

/// Derive the session PDA from owner + ephemeral signer.
/// Seeds: ["session", owner.as_ref(), ephemeral.as_ref()]
pub fn derive_session_pda(owner: &Pubkey, ephemeral: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[b"session", owner.as_ref(), ephemeral.as_ref()],
        program_id,
    )
}

/// Compute Anchor sighash for a method name.
/// Anchor uses the first 8 bytes of SHA-256("global:{method_name}").
fn anchor_sighash(name: &str) -> [u8; 8] {
    let preimage = format!("global:{}", name);
    let hash = solana_sdk::hash::hash(preimage.as_bytes());
    let mut sighash = [0u8; 8];
    sighash.copy_from_slice(&hash.to_bytes()[..8]);
    sighash
}

/// Build a `register_session_key` instruction.
pub fn build_register_session_ix(
    program_id: &Pubkey,
    session_pda: &Pubkey,
    owner: &Pubkey,
    ephemeral_signer: &Pubkey,
    target_program: &Pubkey,
    expires_at: i64,
    spending_limit: u64,
    scopes: Vec<String>,
    token_mint: &Pubkey,
) -> Instruction {
    let sighash = anchor_sighash("register_session_key");

    // Borsh-serialize the instruction arguments
    let mut data = Vec::new();
    data.extend_from_slice(&sighash);
    data.extend_from_slice(target_program.as_ref());
    data.extend_from_slice(&expires_at.to_le_bytes());
    data.extend_from_slice(&spending_limit.to_le_bytes());
    // Borsh Vec<String>: u32 length prefix + each string as u32 len + bytes
    let scopes_len = scopes.len() as u32;
    data.extend_from_slice(&scopes_len.to_le_bytes());
    for scope in &scopes {
        let scope_bytes = scope.as_bytes();
        let scope_len = scope_bytes.len() as u32;
        data.extend_from_slice(&scope_len.to_le_bytes());
        data.extend_from_slice(scope_bytes);
    }
    // token_mint: 32 bytes (Pubkey)
    data.extend_from_slice(token_mint.as_ref());

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*session_pda, false),
            AccountMeta::new(*owner, true),
            AccountMeta::new_readonly(*ephemeral_signer, true),
            AccountMeta::new_readonly(*target_program, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
        ],
        data,
    }
}

/// Build an `execute_payment` instruction.
pub fn build_execute_payment_ix(
    program_id: &Pubkey,
    session_pda: &Pubkey,
    ephemeral_signer: &Pubkey,
    recipient: &Pubkey,
    amount: u64,
    scope: &str,
) -> Instruction {
    let sighash = anchor_sighash("execute_payment");

    let mut data = Vec::new();
    data.extend_from_slice(&sighash);
    data.extend_from_slice(&amount.to_le_bytes());
    // Borsh String: u32 len + bytes
    let scope_bytes = scope.as_bytes();
    let scope_len = scope_bytes.len() as u32;
    data.extend_from_slice(&scope_len.to_le_bytes());
    data.extend_from_slice(scope_bytes);

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*session_pda, false),
            AccountMeta::new(*ephemeral_signer, true),
            AccountMeta::new(*recipient, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
        ],
        data,
    }
}

/// Build an `execute_spl_payment` instruction.
pub fn build_execute_spl_payment_ix(
    program_id: &Pubkey,
    session_pda: &Pubkey,
    ephemeral_signer: &Pubkey,
    source_ata: &Pubkey,
    dest_ata: &Pubkey,
    token_mint: &Pubkey,
    amount: u64,
    scope: &str,
) -> Instruction {
    let sighash = anchor_sighash("execute_spl_payment");

    let mut data = Vec::new();
    data.extend_from_slice(&sighash);
    data.extend_from_slice(&amount.to_le_bytes());
    // Borsh String: u32 len + bytes
    let scope_bytes = scope.as_bytes();
    let scope_len = scope_bytes.len() as u32;
    data.extend_from_slice(&scope_len.to_le_bytes());
    data.extend_from_slice(scope_bytes);

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*session_pda, false),
            AccountMeta::new(*ephemeral_signer, true),
            AccountMeta::new(*source_ata, false),
            AccountMeta::new(*dest_ata, false),
            AccountMeta::new_readonly(*token_mint, false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
        ],
        data,
    }
}

/// Build a `revoke_session` instruction.
pub fn build_revoke_session_ix(
    program_id: &Pubkey,
    session_pda: &Pubkey,
    owner: &Pubkey,
) -> Instruction {
    let sighash = anchor_sighash("revoke_session");

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*session_pda, false),
            AccountMeta::new_readonly(*owner, true),
        ],
        data: sighash.to_vec(),
    }
}

/// Build a `close_session` instruction.
pub fn build_close_session_ix(
    program_id: &Pubkey,
    session_pda: &Pubkey,
    owner: &Pubkey,
) -> Instruction {
    let sighash = anchor_sighash("close_session");

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*session_pda, false),
            AccountMeta::new(*owner, true),
            AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
        ],
        data: sighash.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_session_pda() {
        let owner = Pubkey::new_unique();
        let ephemeral = Pubkey::new_unique();
        let program_id = session_program_id();

        let (pda, bump) = derive_session_pda(&owner, &ephemeral, &program_id);
        assert_ne!(pda, Pubkey::default());
        assert!(bump > 0);

        // Deriving again should give the same result
        let (pda2, bump2) = derive_session_pda(&owner, &ephemeral, &program_id);
        assert_eq!(pda, pda2);
        assert_eq!(bump, bump2);
    }

    #[test]
    fn test_build_register_session_ix() {
        let program_id = session_program_id();
        let owner = Pubkey::new_unique();
        let ephemeral = Pubkey::new_unique();
        let target = system_program::id();
        let (session_pda, _) = derive_session_pda(&owner, &ephemeral, &program_id);

        let ix = build_register_session_ix(
            &program_id,
            &session_pda,
            &owner,
            &ephemeral,
            &target,
            1700000000,
            1_000_000,
            vec!["sol:transfer".to_string()],
            &Pubkey::default(), // SOL session
        );

        assert_eq!(ix.program_id, program_id);
        assert_eq!(ix.accounts.len(), 6);
        assert!(ix.data.len() > 8); // sighash + params
    }

    #[test]
    fn test_build_execute_payment_ix() {
        let program_id = session_program_id();
        let owner = Pubkey::new_unique();
        let ephemeral = Pubkey::new_unique();
        let recipient = Pubkey::new_unique();
        let (session_pda, _) = derive_session_pda(&owner, &ephemeral, &program_id);

        let ix = build_execute_payment_ix(
            &program_id,
            &session_pda,
            &ephemeral,
            &recipient,
            5000,
            "sol:transfer",
        );

        assert_eq!(ix.program_id, program_id);
        assert_eq!(ix.accounts.len(), 5);
    }

    #[test]
    fn test_build_revoke_session_ix() {
        let program_id = session_program_id();
        let owner = Pubkey::new_unique();
        let ephemeral = Pubkey::new_unique();
        let (session_pda, _) = derive_session_pda(&owner, &ephemeral, &program_id);

        let ix = build_revoke_session_ix(&program_id, &session_pda, &owner);

        assert_eq!(ix.program_id, program_id);
        assert_eq!(ix.accounts.len(), 2);
        assert_eq!(ix.data.len(), 8); // sighash only
    }

    #[test]
    fn test_build_close_session_ix() {
        let program_id = session_program_id();
        let owner = Pubkey::new_unique();
        let ephemeral = Pubkey::new_unique();
        let (session_pda, _) = derive_session_pda(&owner, &ephemeral, &program_id);

        let ix = build_close_session_ix(&program_id, &session_pda, &owner);

        assert_eq!(ix.program_id, program_id);
        assert_eq!(ix.accounts.len(), 3);
        assert_eq!(ix.data.len(), 8); // sighash only
    }
}
