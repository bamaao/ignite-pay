use anyhow::Result;
use sha2::{Sha256, Digest};
#[allow(deprecated)]
use solana_sdk::system_program;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signer::Signer,
    transaction::Transaction,
    sysvar::instructions,
};

use crate::pda;

/// Compute the Anchor method discriminator: first 8 bytes of SHA256("global:<method_name>")
fn method_discriminator(method: &str) -> [u8; 8] {
    let preimage = format!("global:{}", method);
    let mut hasher = Sha256::new();
    hasher.update(preimage.as_bytes());
    let hash = hasher.finalize();
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&hash[..8]);
    disc
}

/// Build an `initialize_global` transaction.
pub fn build_initialize_global_tx(
    payer: &dyn Signer,
    token_mint: &Pubkey,
    program_id: &Pubkey,
    recent_blockhash: solana_sdk::hash::Hash,
) -> Result<Transaction> {
    let buyer_pubkey = payer.pubkey();
    let (global_state, _) = pda::derive_global_state_pda(program_id, &buyer_pubkey, token_mint);
    let (vault, _) = pda::derive_global_vault_pda(program_id, &buyer_pubkey, token_mint);

    let disc = method_discriminator("initialize_global");

    let accounts = vec![
        AccountMeta::new(global_state, false),
        AccountMeta::new_readonly(vault, false),
        AccountMeta::new(buyer_pubkey, true),
        AccountMeta::new_readonly(*token_mint, false),
        AccountMeta::new_readonly(system_program::id(), false),
    ];

    let ix = Instruction {
        program_id: *program_id,
        accounts,
        data: disc.to_vec(),
    };

    Ok(Transaction::new_signed_with_payer(
        &[ix],
        Some(&buyer_pubkey),
        &[payer],
        recent_blockhash,
    ))
}

/// Build an `initialize_channel` transaction.
pub fn build_initialize_channel_tx(
    payer: &dyn Signer,
    merchant: &Pubkey,
    token_mint: &Pubkey,
    spending_cap: u64,
    challenge_period: i64,
    dispute_period: i64,
    program_id: &Pubkey,
    recent_blockhash: solana_sdk::hash::Hash,
) -> Result<Transaction> {
    let buyer_pubkey = payer.pubkey();
    let (global_state, _) = pda::derive_global_state_pda(program_id, &buyer_pubkey, token_mint);
    let (channel, _) = pda::derive_channel_pda(program_id, &buyer_pubkey, merchant, token_mint);

    let disc = method_discriminator("initialize_channel");

    // Borsh-serialize args: spending_cap (u64), challenge_period (i64), dispute_period (i64)
    let mut data = Vec::with_capacity(8 + 8 + 8 + 8);
    data.extend_from_slice(&disc);
    data.extend_from_slice(&spending_cap.to_le_bytes());
    data.extend_from_slice(&challenge_period.to_le_bytes());
    data.extend_from_slice(&dispute_period.to_le_bytes());

    let accounts = vec![
        AccountMeta::new(global_state, false),
        AccountMeta::new(channel, false),
        AccountMeta::new(buyer_pubkey, true),
        AccountMeta::new_readonly(*merchant, false),
        AccountMeta::new_readonly(*token_mint, false),
        AccountMeta::new_readonly(system_program::id(), false),
    ];

    let ix = Instruction {
        program_id: *program_id,
        accounts,
        data,
    };

    Ok(Transaction::new_signed_with_payer(
        &[ix],
        Some(&buyer_pubkey),
        &[payer],
        recent_blockhash,
    ))
}

/// Build an `update_spending_cap` transaction.
pub fn build_update_spending_cap_tx(
    payer: &dyn Signer,
    merchant: &Pubkey,
    token_mint: &Pubkey,
    new_spending_cap: u64,
    program_id: &Pubkey,
    recent_blockhash: solana_sdk::hash::Hash,
) -> Result<Transaction> {
    let buyer_pubkey = payer.pubkey();
    let (global_state, _) = pda::derive_global_state_pda(program_id, &buyer_pubkey, token_mint);
    let (channel, _) = pda::derive_channel_pda(program_id, &buyer_pubkey, merchant, token_mint);

    let disc = method_discriminator("update_spending_cap");

    let mut data = Vec::with_capacity(8 + 8);
    data.extend_from_slice(&disc);
    data.extend_from_slice(&new_spending_cap.to_le_bytes());

    let accounts = vec![
        AccountMeta::new(global_state, false),
        AccountMeta::new(channel, false),
        AccountMeta::new(buyer_pubkey, true),
    ];

    let ix = Instruction {
        program_id: *program_id,
        accounts,
        data,
    };

    Ok(Transaction::new_signed_with_payer(
        &[ix],
        Some(&buyer_pubkey),
        &[payer],
        recent_blockhash,
    ))
}

/// Build a `deposit` transaction (to global vault).
pub fn build_deposit_tx(
    buyer: &dyn Signer,
    token_mint: &Pubkey,
    amount: u64,
    program_id: &Pubkey,
    recent_blockhash: solana_sdk::hash::Hash,
) -> Result<Transaction> {
    let buyer_pubkey = buyer.pubkey();
    let (global_state, _) = pda::derive_global_state_pda(program_id, &buyer_pubkey, token_mint);
    let (vault, _) = pda::derive_global_vault_pda(program_id, &buyer_pubkey, token_mint);

    let disc = method_discriminator("deposit");

    let mut data = Vec::with_capacity(8 + 8);
    data.extend_from_slice(&disc);
    data.extend_from_slice(&amount.to_le_bytes());

    let accounts = vec![
        AccountMeta::new(global_state, false),
        AccountMeta::new(vault, false),
        AccountMeta::new(buyer_pubkey, true),
        AccountMeta::new_readonly(system_program::id(), false),
    ];

    let ix = Instruction {
        program_id: *program_id,
        accounts,
        data,
    };

    Ok(Transaction::new_signed_with_payer(
        &[ix],
        Some(&buyer_pubkey),
        &[buyer],
        recent_blockhash,
    ))
}

/// Build a `settle_batch` transaction with two Ed25519 pre-instructions for signature verification.
///
/// The Ed25519 precompiled program instruction format:
/// - 2 bytes: num signatures (u16)
/// - 2 bytes: padding
/// - Per signature: 14-byte header (sig_offset, sig_ix_idx, pubkey_offset, pubkey_ix_idx, msg_offset, msg_size, msg_ix_idx)
///   then data: 64 bytes signature + 32 bytes pubkey + N bytes message
pub fn build_settle_batch_tx(
    merchant: &dyn Signer,
    buyer_pubkey: &Pubkey,
    channel: &Pubkey,
    escrow: &Pubkey,
    token_mint: &Pubkey,
    merkle_root: &[u8; 32],
    total_amount: u64,
    buyer_batch_sig: &[u8; 64],
    merchant_batch_sig: &[u8; 64],
    nonce: u64,
    program_id: &Pubkey,
    recent_blockhash: solana_sdk::hash::Hash,
) -> Result<Transaction> {
    use crate::signing::build_settlement_message;

    let (global_state, _) = pda::derive_global_state_pda(program_id, buyer_pubkey, token_mint);
    let (vault, _) = pda::derive_global_vault_pda(program_id, buyer_pubkey, token_mint);

    let msg_hash = build_settlement_message(merkle_root, total_amount, &channel.to_bytes(), nonce);

    // Build two Ed25519 verification instructions
    let buyer_ed25519_ix = build_ed25519_instruction(
        &buyer_pubkey.to_bytes(),
        buyer_batch_sig,
        &msg_hash,
    );
    let merchant_ed25519_ix = build_ed25519_instruction(
        &merchant.pubkey().to_bytes(),
        merchant_batch_sig,
        &msg_hash,
    );

    let disc = method_discriminator("settle_batch");

    // Borsh args: merkle_root [u8;32], total_amount u64, buyer_batch_sig [u8;64], merchant_batch_sig [u8;64]
    let mut data = Vec::with_capacity(8 + 32 + 8 + 64 + 64);
    data.extend_from_slice(&disc);
    data.extend_from_slice(merkle_root);
    data.extend_from_slice(&total_amount.to_le_bytes());
    data.extend_from_slice(buyer_batch_sig);
    data.extend_from_slice(merchant_batch_sig);

    let accounts = vec![
        AccountMeta::new(global_state, false),
        AccountMeta::new(vault, false),
        AccountMeta::new(*channel, false),
        AccountMeta::new(*escrow, false),
        AccountMeta::new(merchant.pubkey(), true),
        AccountMeta::new_readonly(instructions::id(), false),
        AccountMeta::new_readonly(system_program::id(), false),
    ];

    let settle_ix = Instruction {
        program_id: *program_id,
        accounts,
        data,
    };

    Ok(Transaction::new_signed_with_payer(
        &[buyer_ed25519_ix, merchant_ed25519_ix, settle_ix],
        Some(&merchant.pubkey()),
        &[merchant],
        recent_blockhash,
    ))
}

/// Build an Ed25519 verification instruction with a single signature.
///
/// Layout:
///   [0..2]   num_sigs = 1 (u16 LE)
///   [2..4]   padding = 0
///   [4..18]  header: sig_offset(u16), sig_ix_idx(u16), pubkey_offset(u16), pubkey_ix_idx(u16),
///            msg_offset(u16), msg_size(u16), msg_ix_idx(u16)
///   [18..82] signature (64 bytes)
///   [82..114] pubkey (32 bytes)
///   [114..146] message (32 bytes)
fn build_ed25519_instruction(
    pubkey: &[u8; 32],
    signature: &[u8; 64],
    message: &[u8; 32],
) -> Instruction {
    let num_sigs: u16 = 1;
    let padding: u16 = 0;

    // Offsets are relative to the start of the instruction data
    let data_offset = 4 + 14; // header (4) + per-sig header (14)
    let sig_offset = data_offset as u16;
    let pubkey_offset = (data_offset + 64) as u16;
    let msg_offset = (data_offset + 64 + 32) as u16;
    let msg_size = message.len() as u16;
    let zero: u16 = 0;

    let mut data = Vec::with_capacity(4 + 14 + 64 + 32 + 32);

    // Header
    data.extend_from_slice(&num_sigs.to_le_bytes());
    data.extend_from_slice(&padding.to_le_bytes());

    // Per-signature header (14 bytes)
    data.extend_from_slice(&sig_offset.to_le_bytes());
    data.extend_from_slice(&zero.to_le_bytes()); // sig_ix_idx
    data.extend_from_slice(&pubkey_offset.to_le_bytes());
    data.extend_from_slice(&zero.to_le_bytes()); // pubkey_ix_idx
    data.extend_from_slice(&msg_offset.to_le_bytes());
    data.extend_from_slice(&msg_size.to_le_bytes());
    data.extend_from_slice(&zero.to_le_bytes()); // msg_ix_idx

    // Data
    data.extend_from_slice(signature);
    data.extend_from_slice(pubkey);
    data.extend_from_slice(message);

    Instruction {
        program_id: solana_sdk::ed25519_program::id(),
        accounts: vec![],
        data,
    }
}

/// Build an `optimistic_settle` transaction with a single merchant Ed25519 pre-instruction.
/// Used when the buyer does not cooperate — funds go to escrow and must wait for challenge_period.
pub fn build_optimistic_settle_tx(
    merchant: &dyn Signer,
    buyer_pubkey: &Pubkey,
    channel: &Pubkey,
    escrow: &Pubkey,
    token_mint: &Pubkey,
    merkle_root: &[u8; 32],
    total_amount: u64,
    merchant_batch_sig: &[u8; 64],
    nonce: u64,
    program_id: &Pubkey,
    recent_blockhash: solana_sdk::hash::Hash,
) -> Result<Transaction> {
    use crate::signing::build_settlement_message;

    let (global_state, _) = pda::derive_global_state_pda(program_id, buyer_pubkey, token_mint);
    let (vault, _) = pda::derive_global_vault_pda(program_id, buyer_pubkey, token_mint);

    let msg_hash = build_settlement_message(merkle_root, total_amount, &channel.to_bytes(), nonce);

    let merchant_ed25519_ix = build_ed25519_instruction(
        &merchant.pubkey().to_bytes(),
        merchant_batch_sig,
        &msg_hash,
    );

    let disc = method_discriminator("optimistic_settle");

    // Args: merkle_root [u8;32], total_amount u64, merchant_batch_sig [u8;64]
    let mut data = Vec::with_capacity(8 + 32 + 8 + 64);
    data.extend_from_slice(&disc);
    data.extend_from_slice(merkle_root);
    data.extend_from_slice(&total_amount.to_le_bytes());
    data.extend_from_slice(merchant_batch_sig);

    let accounts = vec![
        AccountMeta::new(global_state, false),
        AccountMeta::new(vault, false),
        AccountMeta::new(*channel, false),
        AccountMeta::new(*escrow, false),
        AccountMeta::new(merchant.pubkey(), true),
        AccountMeta::new_readonly(instructions::id(), false),
        AccountMeta::new_readonly(system_program::id(), false),
    ];

    let settle_ix = Instruction {
        program_id: *program_id,
        accounts,
        data,
    };

    Ok(Transaction::new_signed_with_payer(
        &[merchant_ed25519_ix, settle_ix],
        Some(&merchant.pubkey()),
        &[merchant],
        recent_blockhash,
    ))
}

/// Build a `dispute` transaction.
pub fn build_dispute_tx(
    buyer: &dyn Signer,
    channel: &Pubkey,
    escrow: &Pubkey,
    program_id: &Pubkey,
    recent_blockhash: solana_sdk::hash::Hash,
) -> Result<Transaction> {
    let disc = method_discriminator("dispute");

    let accounts = vec![
        AccountMeta::new(*channel, false),
        AccountMeta::new(*escrow, false),
        AccountMeta::new(buyer.pubkey(), true),
        AccountMeta::new_readonly(system_program::id(), false),
    ];

    let ix = Instruction {
        program_id: *program_id,
        accounts,
        data: disc.to_vec(),
    };

    Ok(Transaction::new_signed_with_payer(
        &[ix],
        Some(&buyer.pubkey()),
        &[buyer],
        recent_blockhash,
    ))
}

/// Build a `release_settlement` transaction.
pub fn build_release_settlement_tx(
    merchant: &dyn Signer,
    channel: &Pubkey,
    escrow: &Pubkey,
    program_id: &Pubkey,
    recent_blockhash: solana_sdk::hash::Hash,
) -> Result<Transaction> {
    let disc = method_discriminator("release_settlement");

    let accounts = vec![
        AccountMeta::new(*channel, false),
        AccountMeta::new(*escrow, false),
        AccountMeta::new(merchant.pubkey(), true),
        AccountMeta::new_readonly(system_program::id(), false),
    ];

    let ix = Instruction {
        program_id: *program_id,
        accounts,
        data: disc.to_vec(),
    };

    Ok(Transaction::new_signed_with_payer(
        &[ix],
        Some(&merchant.pubkey()),
        &[merchant],
        recent_blockhash,
    ))
}

/// Build a `force_release` transaction.
pub fn build_force_release_tx(
    merchant: &dyn Signer,
    channel: &Pubkey,
    escrow: &Pubkey,
    program_id: &Pubkey,
    recent_blockhash: solana_sdk::hash::Hash,
) -> Result<Transaction> {
    let disc = method_discriminator("force_release");

    let accounts = vec![
        AccountMeta::new(*channel, false),
        AccountMeta::new(*escrow, false),
        AccountMeta::new(merchant.pubkey(), true),
        AccountMeta::new_readonly(system_program::id(), false),
    ];

    let ix = Instruction {
        program_id: *program_id,
        accounts,
        data: disc.to_vec(),
    };

    Ok(Transaction::new_signed_with_payer(
        &[ix],
        Some(&merchant.pubkey()),
        &[merchant],
        recent_blockhash,
    ))
}

/// Build a `resolve_dispute` transaction with a Merkle proof.
pub fn build_resolve_dispute_tx(
    buyer: &dyn Signer,
    channel: &Pubkey,
    escrow: &Pubkey,
    voucher_seq: u64,
    voucher_amount: u64,
    buyer_voucher_sig: &[u8; 64],
    sibling_hashes: &[[u8; 32]],
    sibling_sums: &[u64],
    program_id: &Pubkey,
    recent_blockhash: solana_sdk::hash::Hash,
) -> Result<Transaction> {
    let disc = method_discriminator("resolve_dispute");

    // Borsh args: voucher_seq u64, voucher_amount u64, buyer_voucher_sig [u8;64],
    // sibling_hashes Vec<[u8;32]>, sibling_sums Vec<u64>
    let mut data = Vec::new();
    data.extend_from_slice(&disc);
    data.extend_from_slice(&voucher_seq.to_le_bytes());
    data.extend_from_slice(&voucher_amount.to_le_bytes());
    data.extend_from_slice(buyer_voucher_sig);

    // Vec<u8> length prefix for sibling_hashes (Borsh: u32 length + data)
    let hashes_len = sibling_hashes.len() as u32;
    data.extend_from_slice(&hashes_len.to_le_bytes());
    for h in sibling_hashes {
        data.extend_from_slice(h);
    }

    let sums_len = sibling_sums.len() as u32;
    data.extend_from_slice(&sums_len.to_le_bytes());
    for s in sibling_sums {
        data.extend_from_slice(&s.to_le_bytes());
    }

    let accounts = vec![
        AccountMeta::new(*channel, false),
        AccountMeta::new(*escrow, false),
        AccountMeta::new(buyer.pubkey(), true),
        AccountMeta::new_readonly(system_program::id(), false),
    ];

    let ix = Instruction {
        program_id: *program_id,
        accounts,
        data,
    };

    Ok(Transaction::new_signed_with_payer(
        &[ix],
        Some(&buyer.pubkey()),
        &[buyer],
        recent_blockhash,
    ))
}

/// Build a `withdraw` transaction (from global vault).
pub fn build_withdraw_tx(
    buyer: &dyn Signer,
    token_mint: &Pubkey,
    amount: u64,
    program_id: &Pubkey,
    recent_blockhash: solana_sdk::hash::Hash,
) -> Result<Transaction> {
    let buyer_pubkey = buyer.pubkey();
    let (global_state, _) = pda::derive_global_state_pda(program_id, &buyer_pubkey, token_mint);
    let (vault, _) = pda::derive_global_vault_pda(program_id, &buyer_pubkey, token_mint);

    let disc = method_discriminator("withdraw");

    let mut data = Vec::with_capacity(8 + 8);
    data.extend_from_slice(&disc);
    data.extend_from_slice(&amount.to_le_bytes());

    let accounts = vec![
        AccountMeta::new(global_state, false),
        AccountMeta::new(vault, false),
        AccountMeta::new(buyer_pubkey, true),
        AccountMeta::new_readonly(system_program::id(), false),
    ];

    let ix = Instruction {
        program_id: *program_id,
        accounts,
        data,
    };

    Ok(Transaction::new_signed_with_payer(
        &[ix],
        Some(&buyer_pubkey),
        &[buyer],
        recent_blockhash,
    ))
}
