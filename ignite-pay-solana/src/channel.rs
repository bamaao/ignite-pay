//! Instruction builders for the on-chain channel program.
//!
//! Constructs `solana_sdk::instruction::Instruction` for all 10 operations
//! defined in `ignite-pay-program`. Each builder mirrors the exact
//! `#[derive(Accounts)]` layout and argument order from the on-chain program.

use solana_sdk::{
    hash,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    sysvar,
};

// Anchor discriminator: sha256("global:<name>")[..8]
fn anchor_discriminator(name: &str) -> [u8; 8] {
    let preimage = format!("global:{}", name);
    let hash_bytes = hash::hash(preimage.as_bytes()).to_bytes();
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&hash_bytes[..8]);
    disc
}

/// Derive the channel PDA: seeds = `[b"channel", channel_id]`
pub fn derive_channel_pda(channel_id: &[u8; 32], program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"channel", channel_id.as_ref()], program_id)
}

/// Derive the escrow PDA: seeds = `[b"escrow", channel_id]`
pub fn derive_escrow_pda(channel_id: &[u8; 32], program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"escrow", channel_id.as_ref()], program_id)
}

/// Append a `[u8; 32]` to the instruction data buffer.
fn push_bytes32(buf: &mut Vec<u8>, val: &[u8; 32]) {
    buf.extend_from_slice(val);
}

/// Append a `u64` (little-endian) to the instruction data buffer.
fn push_u64(buf: &mut Vec<u8>, val: u64) {
    buf.extend_from_slice(&val.to_le_bytes());
}

/// Append a `u32` (little-endian) to the instruction data buffer.
fn push_u32(buf: &mut Vec<u8>, val: u32) {
    buf.extend_from_slice(&val.to_le_bytes());
}

/// Append a `Pubkey` to the instruction data buffer.
fn push_pubkey(buf: &mut Vec<u8>, pk: &Pubkey) {
    buf.extend_from_slice(pk.as_ref());
}

/// Append a `Vec<[u8; 32]>` as length-prefixed borsh to the instruction data.
fn push_proof(buf: &mut Vec<u8>, proof: &[[u8; 32]]) {
    let len = proof.len() as u32;
    buf.extend_from_slice(&len.to_le_bytes());
    for item in proof {
        buf.extend_from_slice(item);
    }
}

/// Append a `Vec<u8>` as length-prefixed bytes.
fn push_vec(buf: &mut Vec<u8>, data: &[u8]) {
    let len = data.len() as u32;
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(data);
}

// ── 1. OPEN CHANNEL ──

/// Build an `open_channel` instruction.
///
/// On-chain verifies `sig_a` over `channel_id || deposit_a || tree_depth || initial_root`
/// (raw concatenation, NOT the off-chain `state_message` format).
#[allow(clippy::too_many_arguments)]
pub fn build_open_channel_ix(
    program_id: &Pubkey,
    channel_pda: &Pubkey,
    user: &Pubkey,          // signer
    user_pubkey: &Pubkey,   // unchecked
    provider_pubkey: &Pubkey,
    token_mint: &Pubkey,
    vault_a: &Pubkey,
    vault_b: &Pubkey,
    payer: &Pubkey,         // signer, mut
    channel_id: &[u8; 32],
    deposit_a: u64,
    tree_depth: u32,
    open_slot: u64,
    challenge_duration: u64,
    min_challenge_delay: u64,
    initial_root: &[u8; 32],
    sig_a: &[u8; 64],
) -> Instruction {
    let mut data = Vec::with_capacity(8 + 32 + 8 + 4 + 8 + 8 + 8 + 32 + 64);
    data.extend_from_slice(&anchor_discriminator("open_channel"));
    push_bytes32(&mut data, channel_id);
    push_u64(&mut data, deposit_a);
    push_u32(&mut data, tree_depth);
    push_u64(&mut data, open_slot);
    push_u64(&mut data, challenge_duration);
    push_u64(&mut data, min_challenge_delay);
    push_bytes32(&mut data, initial_root);
    data.extend_from_slice(sig_a);

    let accounts = vec![
        AccountMeta::new(*channel_pda, false),
        AccountMeta::new_readonly(*user, true),
        AccountMeta::new_readonly(*user_pubkey, false),
        AccountMeta::new_readonly(*provider_pubkey, false),
        AccountMeta::new_readonly(*token_mint, false),
        AccountMeta::new_readonly(*vault_a, false),
        AccountMeta::new_readonly(*vault_b, false),
        AccountMeta::new(*payer, true),
        AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
        AccountMeta::new_readonly(spl_token::id(), false),
        AccountMeta::new_readonly(sysvar::rent::id(), false),
    ];

    Instruction {
        program_id: *program_id,
        accounts,
        data,
    }
}

// ── 2. FUND CHANNEL ──

/// Build a `fund_channel` instruction.
pub fn build_fund_channel_ix(
    program_id: &Pubkey,
    channel_pda: &Pubkey,
    signer: &Pubkey,        // must be provider, signer
    source_vault: &Pubkey,  // mut
    vault_b: &Pubkey,       // mut
    deposit_b: u64,
) -> Instruction {
    let mut data = Vec::with_capacity(8 + 8);
    data.extend_from_slice(&anchor_discriminator("fund_channel"));
    push_u64(&mut data, deposit_b);

    let accounts = vec![
        AccountMeta::new(*channel_pda, false),
        AccountMeta::new_readonly(*signer, true),
        AccountMeta::new(*source_vault, false),
        AccountMeta::new(*vault_b, false),
        AccountMeta::new_readonly(spl_token::id(), false),
    ];

    Instruction {
        program_id: *program_id,
        accounts,
        data,
    }
}

// ── 3. COOPERATIVE SETTLE ──

/// Build a `cooperative_settle` instruction.
pub fn build_cooperative_settle_ix(
    program_id: &Pubkey,
    channel_pda: &Pubkey,   // mut
    sequence: u64,
    root: &[u8; 32],
    settle_window: u64,
    sig_a: &[u8; 64],
    sig_b: &[u8; 64],
) -> Instruction {
    let mut data = Vec::with_capacity(8 + 8 + 32 + 8 + 64 + 64);
    data.extend_from_slice(&anchor_discriminator("cooperative_settle"));
    push_u64(&mut data, sequence);
    push_bytes32(&mut data, root);
    push_u64(&mut data, settle_window);
    data.extend_from_slice(sig_a);
    data.extend_from_slice(sig_b);

    let accounts = vec![
        AccountMeta::new(*channel_pda, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
    ];

    Instruction {
        program_id: *program_id,
        accounts,
        data,
    }
}

// ── 4. TRIGGER CHALLENGE ──

/// Build a `trigger_challenge` instruction.
pub fn build_trigger_challenge_ix(
    program_id: &Pubkey,
    channel_pda: &Pubkey,   // mut
    challenger: &Pubkey,    // signer
    submitted_root: &[u8; 32],
    submitted_sequence: u64,
    challenger_signature: &[u8; 64],
) -> Instruction {
    let mut data = Vec::with_capacity(8 + 32 + 8 + 64);
    data.extend_from_slice(&anchor_discriminator("trigger_challenge"));
    push_bytes32(&mut data, submitted_root);
    push_u64(&mut data, submitted_sequence);
    data.extend_from_slice(challenger_signature);

    let accounts = vec![
        AccountMeta::new(*channel_pda, false),
        AccountMeta::new_readonly(*challenger, true),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
    ];

    Instruction {
        program_id: *program_id,
        accounts,
        data,
    }
}

// ── 5. SUBMIT COUNTER STATE ──

/// Build a `submit_counter_state` instruction.
pub fn build_submit_counter_state_ix(
    program_id: &Pubkey,
    channel_pda: &Pubkey,   // mut
    sequence: u64,
    root: &[u8; 32],
    sig_a: &[u8; 64],
    sig_b: &[u8; 64],
) -> Instruction {
    let mut data = Vec::with_capacity(8 + 8 + 32 + 64 + 64);
    data.extend_from_slice(&anchor_discriminator("submit_counter_state"));
    push_u64(&mut data, sequence);
    push_bytes32(&mut data, root);
    data.extend_from_slice(sig_a);
    data.extend_from_slice(sig_b);

    let accounts = vec![
        AccountMeta::new(*channel_pda, false),
    ];

    Instruction {
        program_id: *program_id,
        accounts,
        data,
    }
}

// ── 6. SETTLE AFTER TIMEOUT ──

/// Build a `settle_after_timeout` instruction.
pub fn build_settle_after_timeout_ix(
    program_id: &Pubkey,
    channel_pda: &Pubkey,   // mut
    settle_window: u64,
) -> Instruction {
    let mut data = Vec::with_capacity(8 + 8);
    data.extend_from_slice(&anchor_discriminator("settle_after_timeout"));
    push_u64(&mut data, settle_window);

    let accounts = vec![
        AccountMeta::new(*channel_pda, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
    ];

    Instruction {
        program_id: *program_id,
        accounts,
        data,
    }
}

// ── 7. CLAIM ──

/// Build a `claim` instruction.
#[allow(clippy::too_many_arguments)]
pub fn build_claim_ix(
    program_id: &Pubkey,
    channel_pda: &Pubkey,
    claimer: &Pubkey,       // signer
    vault: &Pubkey,         // mut (destination token account)
    escrow_vault: &Pubkey,  // mut (PDA escrow)
    leaf_index: u32,
    claim_amount: u64,
    leaf_owner: &Pubkey,
    leaf_hash: &[u8; 32],
    proof: &[[u8; 32]],
    leaf_data: &[u8],
    claimer_signature: &[u8; 64],
) -> Instruction {
    let mut data = Vec::with_capacity(256);
    data.extend_from_slice(&anchor_discriminator("claim"));
    push_u32(&mut data, leaf_index);
    push_u64(&mut data, claim_amount);
    push_pubkey(&mut data, leaf_owner);
    push_bytes32(&mut data, leaf_hash);
    push_proof(&mut data, proof);
    push_vec(&mut data, leaf_data);
    data.extend_from_slice(claimer_signature);

    let accounts = vec![
        AccountMeta::new(*channel_pda, false),
        AccountMeta::new_readonly(*claimer, true),
        AccountMeta::new(*vault, false),
        AccountMeta::new(*escrow_vault, false),
        AccountMeta::new_readonly(spl_token::id(), false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
    ];

    Instruction {
        program_id: *program_id,
        accounts,
        data,
    }
}

// ── 8. VERIFY HTLC ──

/// Build a `verify_htlc` instruction.
#[allow(clippy::too_many_arguments)]
pub fn build_verify_htlc_ix(
    program_id: &Pubkey,
    channel_pda: &Pubkey,
    claimer: &Pubkey,       // signer
    vault: &Pubkey,         // mut
    escrow_vault: &Pubkey,  // mut
    leaf_index: u32,
    preimage: &[u8; 32],
    hash_lock: &[u8; 32],
    leaf_amount: u64,
    beneficiary: &Pubkey,
    leaf_hash: &[u8; 32],
    proof: &[[u8; 32]],
    timelock_slot: u64,
    leaf_data: &[u8],
    claimer_signature: &[u8; 64],
) -> Instruction {
    let mut data = Vec::with_capacity(256);
    data.extend_from_slice(&anchor_discriminator("verify_htlc"));
    push_u32(&mut data, leaf_index);
    data.extend_from_slice(preimage);
    push_bytes32(&mut data, hash_lock);
    push_u64(&mut data, leaf_amount);
    push_pubkey(&mut data, beneficiary);
    push_bytes32(&mut data, leaf_hash);
    push_proof(&mut data, proof);
    push_u64(&mut data, timelock_slot);
    push_vec(&mut data, leaf_data);
    data.extend_from_slice(claimer_signature);

    let accounts = vec![
        AccountMeta::new(*channel_pda, false),
        AccountMeta::new_readonly(*claimer, true),
        AccountMeta::new(*vault, false),
        AccountMeta::new(*escrow_vault, false),
        AccountMeta::new_readonly(spl_token::id(), false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
    ];

    Instruction {
        program_id: *program_id,
        accounts,
        data,
    }
}

// ── 9. HTLC REFUND ──

/// Build an `htlc_refund` instruction.
#[allow(clippy::too_many_arguments)]
pub fn build_htlc_refund_ix(
    program_id: &Pubkey,
    channel_pda: &Pubkey,
    claimer: &Pubkey,       // signer
    vault: &Pubkey,         // mut
    escrow_vault: &Pubkey,  // mut
    leaf_index: u32,
    timelock_slot: u64,
    leaf_amount: u64,
    leaf_owner: &Pubkey,
    leaf_hash: &[u8; 32],
    proof: &[[u8; 32]],
    leaf_data: &[u8],
    claimer_signature: &[u8; 64],
) -> Instruction {
    let mut data = Vec::with_capacity(256);
    data.extend_from_slice(&anchor_discriminator("htlc_refund"));
    push_u32(&mut data, leaf_index);
    push_u64(&mut data, timelock_slot);
    push_u64(&mut data, leaf_amount);
    push_pubkey(&mut data, leaf_owner);
    push_bytes32(&mut data, leaf_hash);
    push_proof(&mut data, proof);
    push_vec(&mut data, leaf_data);
    data.extend_from_slice(claimer_signature);

    let accounts = vec![
        AccountMeta::new(*channel_pda, false),
        AccountMeta::new_readonly(*claimer, true),
        AccountMeta::new(*vault, false),
        AccountMeta::new(*escrow_vault, false),
        AccountMeta::new_readonly(spl_token::id(), false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
    ];

    Instruction {
        program_id: *program_id,
        accounts,
        data,
    }
}

// ── 10. FINALIZE SETTLEMENT ──

/// Build a `finalize_settlement` instruction.
pub fn build_finalize_settlement_ix(
    program_id: &Pubkey,
    channel_pda: &Pubkey,
    caller: &Pubkey,        // signer
    vault_a: &Pubkey,       // mut
    vault_b: &Pubkey,       // mut
    escrow_vault: &Pubkey,  // mut
    caller_signature: &[u8; 64],
) -> Instruction {
    let mut data = Vec::with_capacity(8 + 64);
    data.extend_from_slice(&anchor_discriminator("finalize_settlement"));
    data.extend_from_slice(caller_signature);

    let accounts = vec![
        AccountMeta::new(*channel_pda, false),
        AccountMeta::new_readonly(*caller, true),
        AccountMeta::new(*vault_a, false),
        AccountMeta::new(*vault_b, false),
        AccountMeta::new(*escrow_vault, false),
        AccountMeta::new_readonly(spl_token::id(), false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
    ];

    Instruction {
        program_id: *program_id,
        accounts,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_program_id() -> Pubkey {
        Pubkey::new_from_array([1u8; 32])
    }

    #[test]
    fn test_derive_channel_pda_deterministic() {
        let program_id = test_program_id();
        let channel_id = [42u8; 32];
        let (pda1, bump1) = derive_channel_pda(&channel_id, &program_id);
        let (pda2, bump2) = derive_channel_pda(&channel_id, &program_id);
        assert_eq!(pda1, pda2);
        assert_eq!(bump1, bump2);
    }

    #[test]
    fn test_derive_escrow_pda_deterministic() {
        let program_id = test_program_id();
        let channel_id = [99u8; 32];
        let (pda1, bump1) = derive_escrow_pda(&channel_id, &program_id);
        let (pda2, bump2) = derive_escrow_pda(&channel_id, &program_id);
        assert_eq!(pda1, pda2);
        assert_eq!(bump1, bump2);
    }

    #[test]
    fn test_derive_channel_vs_escrow_different() {
        let program_id = test_program_id();
        let channel_id = [7u8; 32];
        let (ch_pda, _) = derive_channel_pda(&channel_id, &program_id);
        let (esc_pda, _) = derive_escrow_pda(&channel_id, &program_id);
        assert_ne!(ch_pda, esc_pda);
    }

    #[test]
    fn test_anchor_discriminator_deterministic() {
        let d1 = anchor_discriminator("open_channel");
        let d2 = anchor_discriminator("open_channel");
        assert_eq!(d1, d2);
    }

    #[test]
    fn test_anchor_discriminator_different_names() {
        let d1 = anchor_discriminator("open_channel");
        let d2 = anchor_discriminator("fund_channel");
        assert_ne!(d1, d2);
    }

    #[test]
    fn test_anchor_discriminator_length() {
        let d = anchor_discriminator("claim");
        assert_eq!(d.len(), 8);
    }

    #[test]
    fn test_build_open_channel_ix_data_size() {
        let program_id = test_program_id();
        let channel_id = [1u8; 32];
        let (channel_pda, _) = derive_channel_pda(&channel_id, &program_id);
        let user = Pubkey::new_unique();
        let user_pk = Pubkey::new_unique();
        let provider_pk = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let vault_a = Pubkey::new_unique();
        let vault_b = Pubkey::new_unique();
        let payer = Pubkey::new_unique();

        let ix = build_open_channel_ix(
            &program_id,
            &channel_pda,
            &user,
            &user_pk,
            &provider_pk,
            &mint,
            &vault_a,
            &vault_b,
            &payer,
            &channel_id,
            1000,
            4,
            100,
            5000,
            1000,
            &[2u8; 32],
            &[3u8; 64],
        );

        // discriminator(8) + channel_id(32) + deposit_a(8) + tree_depth(4) + open_slot(8)
        // + challenge_duration(8) + min_challenge_delay(8) + initial_root(32) + sig_a(64) = 172
        assert_eq!(ix.data.len(), 172);
        assert_eq!(ix.accounts.len(), 11);
        assert_eq!(ix.program_id, program_id);
    }

    #[test]
    fn test_build_fund_channel_ix() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let source_vault = Pubkey::new_unique();
        let vault_b = Pubkey::new_unique();

        let ix = build_fund_channel_ix(
            &program_id, &channel_pda, &signer, &source_vault, &vault_b, 5000,
        );

        assert_eq!(ix.data.len(), 16); // disc(8) + deposit_b(8)
        assert_eq!(ix.accounts.len(), 5);
    }

    #[test]
    fn test_build_cooperative_settle_ix() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();

        let ix = build_cooperative_settle_ix(
            &program_id, &channel_pda, 10, &[1u8; 32], 10000, &[2u8; 64], &[3u8; 64],
        );

        // disc(8) + sequence(8) + root(32) + settle_window(8) + sig_a(64) + sig_b(64) = 184
        assert_eq!(ix.data.len(), 184);
        assert_eq!(ix.accounts.len(), 2);
    }

    #[test]
    fn test_build_trigger_challenge_ix() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();
        let challenger = Pubkey::new_unique();

        let ix = build_trigger_challenge_ix(
            &program_id, &channel_pda, &challenger, &[1u8; 32], 5, &[2u8; 64],
        );

        // disc(8) + submitted_root(32) + submitted_sequence(8) + sig(64) = 112
        assert_eq!(ix.data.len(), 112);
        assert_eq!(ix.accounts.len(), 3);
    }

    #[test]
    fn test_build_submit_counter_state_ix() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();

        let ix = build_submit_counter_state_ix(
            &program_id, &channel_pda, 10, &[1u8; 32], &[2u8; 64], &[3u8; 64],
        );

        // disc(8) + sequence(8) + root(32) + sig_a(64) + sig_b(64) = 176
        assert_eq!(ix.data.len(), 176);
        assert_eq!(ix.accounts.len(), 1);
    }

    #[test]
    fn test_build_settle_after_timeout_ix() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();

        let ix = build_settle_after_timeout_ix(&program_id, &channel_pda, 10000);

        assert_eq!(ix.data.len(), 16); // disc(8) + settle_window(8)
        assert_eq!(ix.accounts.len(), 2);
    }

    #[test]
    fn test_build_claim_ix() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();
        let claimer = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let escrow = Pubkey::new_unique();
        let leaf_owner = Pubkey::new_unique();
        let proof = vec![[4u8; 32], [5u8; 32]];
        let leaf_data = vec![1u8; 41];

        let ix = build_claim_ix(
            &program_id, &channel_pda, &claimer, &vault, &escrow,
            0, 500, &leaf_owner, &[1u8; 32], &proof, &leaf_data, &[2u8; 64],
        );

        assert_eq!(ix.accounts.len(), 6);
        assert_eq!(ix.program_id, program_id);
    }

    #[test]
    fn test_build_verify_htlc_ix() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();
        let claimer = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let escrow = Pubkey::new_unique();
        let beneficiary = Pubkey::new_unique();
        let proof = vec![[4u8; 32]];
        let leaf_data = vec![1u8; 50];

        let ix = build_verify_htlc_ix(
            &program_id, &channel_pda, &claimer, &vault, &escrow,
            1, &[10u8; 32], &[11u8; 32], 1000, &beneficiary,
            &[1u8; 32], &proof, 5000, &leaf_data, &[2u8; 64],
        );

        assert_eq!(ix.accounts.len(), 6);
    }

    #[test]
    fn test_build_htlc_refund_ix() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();
        let claimer = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let escrow = Pubkey::new_unique();
        let leaf_owner = Pubkey::new_unique();
        let proof = vec![];
        let leaf_data = vec![1u8; 41];

        let ix = build_htlc_refund_ix(
            &program_id, &channel_pda, &claimer, &vault, &escrow,
            2, 100, 500, &leaf_owner, &[1u8; 32], &proof, &leaf_data, &[3u8; 64],
        );

        assert_eq!(ix.accounts.len(), 6);
    }

    #[test]
    fn test_build_finalize_settlement_ix() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();
        let caller = Pubkey::new_unique();
        let vault_a = Pubkey::new_unique();
        let vault_b = Pubkey::new_unique();
        let escrow = Pubkey::new_unique();

        let ix = build_finalize_settlement_ix(
            &program_id, &channel_pda, &caller, &vault_a, &vault_b, &escrow, &[1u8; 64],
        );

        assert_eq!(ix.data.len(), 72); // disc(8) + sig(64)
        assert_eq!(ix.accounts.len(), 7);
    }

    // ── Deep data-content tests ──

    #[test]
    fn test_open_channel_data_encoding() {
        let program_id = test_program_id();
        let channel_id = [0xAA; 32];
        let (channel_pda, _) = derive_channel_pda(&channel_id, &program_id);
        let user = Pubkey::new_unique();
        let user_pk = Pubkey::new_unique();
        let provider_pk = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let vault_a = Pubkey::new_unique();
        let vault_b = Pubkey::new_unique();
        let payer = Pubkey::new_unique();
        let initial_root = [0xBB; 32];
        let sig_a = [0xCC; 64];

        let ix = build_open_channel_ix(
            &program_id, &channel_pda, &user, &user_pk, &provider_pk,
            &mint, &vault_a, &vault_b, &payer,
            &channel_id, 1000, 4, 200, 5000, 1000, &initial_root, &sig_a,
        );

        // Verify discriminator
        let disc = anchor_discriminator("open_channel");
        assert_eq!(&ix.data[0..8], &disc);

        // Verify channel_id at offset 8
        assert_eq!(&ix.data[8..40], &channel_id);

        // Verify deposit_a (1000) at offset 40
        assert_eq!(u64::from_le_bytes(ix.data[40..48].try_into().unwrap()), 1000);

        // Verify tree_depth (4) at offset 48
        assert_eq!(u32::from_le_bytes(ix.data[48..52].try_into().unwrap()), 4);

        // Verify open_slot (200) at offset 52
        assert_eq!(u64::from_le_bytes(ix.data[52..60].try_into().unwrap()), 200);

        // Verify challenge_duration (5000) at offset 60
        assert_eq!(u64::from_le_bytes(ix.data[60..68].try_into().unwrap()), 5000);

        // Verify min_challenge_delay (1000) at offset 68
        assert_eq!(u64::from_le_bytes(ix.data[68..76].try_into().unwrap()), 1000);

        // Verify initial_root at offset 76
        assert_eq!(&ix.data[76..108], &initial_root);

        // Verify sig_a at offset 108
        assert_eq!(&ix.data[108..172], &sig_a);

        // Verify account ordering: channel_pda, user(signer), user_pk, provider_pk, mint, vault_a, vault_b, payer(signer+mut), system_program, spl_token, rent
        assert_eq!(ix.accounts[0].pubkey, channel_pda);
        assert!(ix.accounts[0].is_writable);
        assert!(!ix.accounts[0].is_signer);

        assert_eq!(ix.accounts[1].pubkey, user);
        assert!(ix.accounts[1].is_signer);

        assert_eq!(ix.accounts[2].pubkey, user_pk);
        assert!(!ix.accounts[2].is_signer);

        assert_eq!(ix.accounts[3].pubkey, provider_pk);
        assert_eq!(ix.accounts[4].pubkey, mint);
        assert_eq!(ix.accounts[5].pubkey, vault_a);
        assert_eq!(ix.accounts[6].pubkey, vault_b);

        assert_eq!(ix.accounts[7].pubkey, payer);
        assert!(ix.accounts[7].is_signer);
        assert!(ix.accounts[7].is_writable);
    }

    #[test]
    fn test_fund_channel_data_encoding() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let source_vault = Pubkey::new_unique();
        let vault_b = Pubkey::new_unique();

        let ix = build_fund_channel_ix(
            &program_id, &channel_pda, &signer, &source_vault, &vault_b, 999999,
        );

        let disc = anchor_discriminator("fund_channel");
        assert_eq!(&ix.data[0..8], &disc);
        assert_eq!(u64::from_le_bytes(ix.data[8..16].try_into().unwrap()), 999999);

        // signer must be signer, source_vault and vault_b must be writable
        assert!(ix.accounts[1].is_signer); // signer
        assert!(ix.accounts[2].is_writable); // source_vault
        assert!(ix.accounts[3].is_writable); // vault_b
    }

    #[test]
    fn test_cooperative_settle_data_encoding() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();
        let root = [0x11; 32];
        let sig_a = [0x22; 64];
        let sig_b = [0x33; 64];

        let ix = build_cooperative_settle_ix(
            &program_id, &channel_pda, 42, &root, 10000, &sig_a, &sig_b,
        );

        let disc = anchor_discriminator("cooperative_settle");
        assert_eq!(&ix.data[0..8], &disc);
        assert_eq!(u64::from_le_bytes(ix.data[8..16].try_into().unwrap()), 42);
        assert_eq!(&ix.data[16..48], &root);
        assert_eq!(u64::from_le_bytes(ix.data[48..56].try_into().unwrap()), 10000);
        assert_eq!(&ix.data[56..120], &sig_a);
        assert_eq!(&ix.data[120..184], &sig_b);

        // channel_pda is writable, clock is readonly
        assert!(ix.accounts[0].is_writable);
        assert_eq!(ix.accounts[0].pubkey, channel_pda);
        assert_eq!(ix.accounts[1].pubkey, sysvar::clock::id());
    }

    #[test]
    fn test_trigger_challenge_data_encoding() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();
        let challenger = Pubkey::new_unique();
        let submitted_root = [0xDD; 32];
        let challenger_sig = [0xEE; 64];

        let ix = build_trigger_challenge_ix(
            &program_id, &channel_pda, &challenger, &submitted_root, 77, &challenger_sig,
        );

        let disc = anchor_discriminator("trigger_challenge");
        assert_eq!(&ix.data[0..8], &disc);
        assert_eq!(&ix.data[8..40], &submitted_root);
        assert_eq!(u64::from_le_bytes(ix.data[40..48].try_into().unwrap()), 77);
        assert_eq!(&ix.data[48..112], &challenger_sig);

        assert_eq!(ix.accounts[0].pubkey, channel_pda);
        assert!(ix.accounts[0].is_writable);
        assert_eq!(ix.accounts[1].pubkey, challenger);
        assert!(ix.accounts[1].is_signer);
    }

    #[test]
    fn test_submit_counter_state_data_encoding() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();
        let root = [0xFF; 32];
        let sig_a = [0x11; 64];
        let sig_b = [0x22; 64];

        let ix = build_submit_counter_state_ix(
            &program_id, &channel_pda, 99, &root, &sig_a, &sig_b,
        );

        let disc = anchor_discriminator("submit_counter_state");
        assert_eq!(&ix.data[0..8], &disc);
        assert_eq!(u64::from_le_bytes(ix.data[8..16].try_into().unwrap()), 99);
        assert_eq!(&ix.data[16..48], &root);
        assert_eq!(&ix.data[48..112], &sig_a);
        assert_eq!(&ix.data[112..176], &sig_b);

        // Only channel_pda account
        assert_eq!(ix.accounts.len(), 1);
        assert!(ix.accounts[0].is_writable);
    }

    #[test]
    fn test_settle_after_timeout_data_encoding() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();

        let ix = build_settle_after_timeout_ix(&program_id, &channel_pda, 12345);

        let disc = anchor_discriminator("settle_after_timeout");
        assert_eq!(&ix.data[0..8], &disc);
        assert_eq!(u64::from_le_bytes(ix.data[8..16].try_into().unwrap()), 12345);
    }

    #[test]
    fn test_claim_ix_proof_encoding() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();
        let claimer = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let escrow = Pubkey::new_unique();
        let leaf_owner = Pubkey::new_unique();
        let leaf_hash = [0xAB; 32];
        let proof = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        let leaf_data = vec![0xFF; 10];
        let claimer_sig = [0xCC; 64];

        let ix = build_claim_ix(
            &program_id, &channel_pda, &claimer, &vault, &escrow,
            5, 7500, &leaf_owner, &leaf_hash, &proof, &leaf_data, &claimer_sig,
        );

        let disc = anchor_discriminator("claim");
        assert_eq!(&ix.data[0..8], &disc);

        // leaf_index at offset 8
        assert_eq!(u32::from_le_bytes(ix.data[8..12].try_into().unwrap()), 5);

        // claim_amount at offset 12
        assert_eq!(u64::from_le_bytes(ix.data[12..20].try_into().unwrap()), 7500);

        // leaf_owner at offset 20
        assert_eq!(&ix.data[20..52], leaf_owner.as_ref());

        // leaf_hash at offset 52
        assert_eq!(&ix.data[52..84], &leaf_hash);

        // proof: length(u32) + 3 * [u8;32] at offset 84
        let proof_len = u32::from_le_bytes(ix.data[84..88].try_into().unwrap());
        assert_eq!(proof_len, 3);
        assert_eq!(&ix.data[88..120], &[1u8; 32]);
        assert_eq!(&ix.data[120..152], &[2u8; 32]);
        assert_eq!(&ix.data[152..184], &[3u8; 32]);

        // Verify account metas
        assert_eq!(ix.accounts[0].pubkey, channel_pda);
        assert!(ix.accounts[0].is_writable);
        assert_eq!(ix.accounts[1].pubkey, claimer);
        assert!(ix.accounts[1].is_signer);
        assert!(ix.accounts[2].is_writable); // vault
        assert!(ix.accounts[3].is_writable); // escrow
    }

    #[test]
    fn test_verify_htlc_proof_encoding() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();
        let claimer = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let escrow = Pubkey::new_unique();
        let beneficiary = Pubkey::new_unique();
        let preimage = [0xAA; 32];
        let hash_lock = [0xBB; 32];
        let leaf_hash = [0xCC; 32];
        let proof: Vec<[u8; 32]> = vec![];
        let leaf_data: Vec<u8> = vec![];
        let claimer_sig = [0xDD; 64];

        let ix = build_verify_htlc_ix(
            &program_id, &channel_pda, &claimer, &vault, &escrow,
            3, &preimage, &hash_lock, 2000, &beneficiary,
            &leaf_hash, &proof, 100, &leaf_data, &claimer_sig,
        );

        let disc = anchor_discriminator("verify_htlc");
        assert_eq!(&ix.data[0..8], &disc);

        // leaf_index
        assert_eq!(u32::from_le_bytes(ix.data[8..12].try_into().unwrap()), 3);

        // preimage
        assert_eq!(&ix.data[12..44], &preimage);

        // hash_lock
        assert_eq!(&ix.data[44..76], &hash_lock);

        // leaf_amount
        assert_eq!(u64::from_le_bytes(ix.data[76..84].try_into().unwrap()), 2000);

        // beneficiary
        assert_eq!(&ix.data[84..116], beneficiary.as_ref());
    }

    #[test]
    fn test_htlc_refund_data_encoding() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();
        let claimer = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let escrow = Pubkey::new_unique();
        let leaf_owner = Pubkey::new_unique();
        let leaf_hash = [0x11; 32];
        let claimer_sig = [0x22; 64];

        let ix = build_htlc_refund_ix(
            &program_id, &channel_pda, &claimer, &vault, &escrow,
            7, 300, 5000, &leaf_owner, &leaf_hash, &[], &[], &claimer_sig,
        );

        let disc = anchor_discriminator("htlc_refund");
        assert_eq!(&ix.data[0..8], &disc);

        // leaf_index
        assert_eq!(u32::from_le_bytes(ix.data[8..12].try_into().unwrap()), 7);

        // timelock_slot
        assert_eq!(u64::from_le_bytes(ix.data[12..20].try_into().unwrap()), 300);

        // leaf_amount
        assert_eq!(u64::from_le_bytes(ix.data[20..28].try_into().unwrap()), 5000);

        // leaf_owner
        assert_eq!(&ix.data[28..60], leaf_owner.as_ref());
    }

    #[test]
    fn test_finalize_settlement_accounts() {
        let program_id = test_program_id();
        let channel_pda = Pubkey::new_unique();
        let caller = Pubkey::new_unique();
        let vault_a = Pubkey::new_unique();
        let vault_b = Pubkey::new_unique();
        let escrow = Pubkey::new_unique();
        let caller_sig = [0xFF; 64];

        let ix = build_finalize_settlement_ix(
            &program_id, &channel_pda, &caller, &vault_a, &vault_b, &escrow, &caller_sig,
        );

        // Verify all accounts are present in correct order
        assert_eq!(ix.accounts[0].pubkey, channel_pda);
        assert!(ix.accounts[0].is_writable);
        assert_eq!(ix.accounts[1].pubkey, caller);
        assert!(ix.accounts[1].is_signer);
        assert_eq!(ix.accounts[2].pubkey, vault_a);
        assert!(ix.accounts[2].is_writable);
        assert_eq!(ix.accounts[3].pubkey, vault_b);
        assert!(ix.accounts[3].is_writable);
        assert_eq!(ix.accounts[4].pubkey, escrow);
        assert!(ix.accounts[4].is_writable);
        assert_eq!(ix.accounts[5].pubkey, spl_token::id());
        assert_eq!(ix.accounts[6].pubkey, sysvar::clock::id());

        // Verify discriminator + sig
        let disc = anchor_discriminator("finalize_settlement");
        assert_eq!(&ix.data[0..8], &disc);
        assert_eq!(&ix.data[8..72], &caller_sig);
    }

    #[test]
    fn test_different_channel_ids_different_pda() {
        let program_id = test_program_id();
        let id1 = [1u8; 32];
        let id2 = [2u8; 32];
        let (pda1, _) = derive_channel_pda(&id1, &program_id);
        let (pda2, _) = derive_channel_pda(&id2, &program_id);
        assert_ne!(pda1, pda2);
    }

    #[test]
    fn test_all_discriminators_unique() {
        let names = [
            "open_channel", "fund_channel", "cooperative_settle",
            "trigger_challenge", "submit_counter_state", "settle_after_timeout",
            "claim", "verify_htlc", "htlc_refund", "finalize_settlement",
        ];
        let discriminators: Vec<[u8; 8]> = names.iter().map(|n| anchor_discriminator(n)).collect();

        for i in 0..discriminators.len() {
            for j in (i + 1)..discriminators.len() {
                assert_ne!(
                    discriminators[i], discriminators[j],
                    "discriminators for {} and {} collide",
                    names[i], names[j],
                );
            }
        }
    }
}
