pub mod state;
pub mod error;
pub mod utils;

use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
use anchor_lang::solana_program::program::invoke_signed;
use error::DidError;
use state::{anchor_sighash, compute_merchant_leaf_hash, DidConfig};
use utils::did::extract_pubkey_from_did;
use utils::ed25519::verify_ed25519_signature;

declare_id!("D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D");

/// spl-account-compression program ID (cmtDvXumGCrqC1Age74AVPhSRVXJMd8PJS91L8KbNCK)
fn spl_account_compression_id() -> Pubkey {
    Pubkey::new_from_array([
        0x43, 0x4d, 0x66, 0xa9, 0x42, 0xd0, 0x44, 0x3b,
        0x67, 0x30, 0x28, 0x64, 0x38, 0xf1, 0x65, 0xb8,
        0x47, 0x99, 0xb0, 0x40, 0x7a, 0xdd, 0x0d, 0x3b,
        0xc7, 0xa2, 0x60, 0xe2, 0x93, 0xb9, 0x09, 0x3e,
    ])
}

/// spl-noop program ID (noopb9bkMVfRPU8AsbpTUg8AQkHtKwMYZiXCjH3k9kV)
fn spl_noop_id() -> Pubkey {
    Pubkey::new_from_array([
        0x9e, 0x13, 0x13, 0x51, 0x74, 0x24, 0x7a, 0x77,
        0x69, 0x8b, 0xa2, 0x4e, 0x6d, 0x36, 0xd6, 0x02,
        0x1b, 0x62, 0x4b, 0xc2, 0x4f, 0x30, 0x8e, 0x49,
        0x10, 0x83, 0xdd, 0xd6, 0xf2, 0x12, 0x20, 0xac,
    ])
}

/// Verify a Merkle proof: walk from leaf to root, hashing at each level.
pub fn verify_merkle_proof(
    leaf_hash: &[u8; 32],
    proof: &[[u8; 32]],
    root: &[u8; 32],
) -> bool {
    let mut current = *leaf_hash;
    for sibling in proof {
        let (left, right) = if current < *sibling {
            (current, *sibling)
        } else {
            (*sibling, current)
        };
        current = state::hash_pair(&left, &right);
    }
    current == *root
}

#[program]
pub mod ignite_pay_did_program {
    use super::*;

    // ─── 1. initialize_tree ───

    #[derive(Accounts)]
    pub struct InitializeTree<'info> {
        #[account(
            init,
            payer = payer,
            space = DidConfig::LEN,
            seeds = [b"did-config"],
            bump,
        )]
        pub did_config: Account<'info, DidConfig>,
        /// CHECK: Merkle tree account created externally (via spl-account-compression)
        pub merkle_tree: UncheckedAccount<'info>,
        #[account(mut)]
        pub payer: Signer<'info>,
        pub system_program: Program<'info, System>,
    }

    pub fn initialize_tree(
        ctx: Context<InitializeTree>,
        platform_authority: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.did_config;
        config.platform_authority = platform_authority;
        config.merkle_tree = ctx.accounts.merkle_tree.key();
        config.bump = ctx.bumps.did_config;
        Ok(())
    }

    // ─── 2. register_merchant ───

    #[derive(Accounts)]
    #[instruction(
        merchant_did: String,
        active_pubkey: Pubkey,
        platform_vc_hash: [u8; 32],
        signature: [u8; 64],
    )]
    pub struct RegisterMerchant<'info> {
        #[account(seeds = [b"did-config"], bump = did_config.bump)]
        pub did_config: Account<'info, DidConfig>,
        /// CHECK: Tree authority PDA
        #[account(
            seeds = [b"did-tree-authority", did_config.merkle_tree.as_ref()],
            bump,
        )]
        pub tree_authority: UncheckedAccount<'info>,
        /// CHECK: Merkle tree account managed by spl-account-compression
        #[account(mut)]
        pub merkle_tree: UncheckedAccount<'info>,
        #[account(mut)]
        pub payer: Signer<'info>,
        /// CHECK: spl-noop program
        pub noop_program: UncheckedAccount<'info>,
        pub clock: Sysvar<'info, Clock>,
        pub system_program: Program<'info, System>,
    }

    pub fn register_merchant(
        ctx: Context<RegisterMerchant>,
        merchant_did: String,
        active_pubkey: Pubkey,
        platform_vc_hash: [u8; 32],
        signature: [u8; 64],
    ) -> Result<()> {
        let pk_bytes = extract_pubkey_from_did(&merchant_did)
            .ok_or(DidError::InvalidDidFormat)?;
        let did_pubkey = Pubkey::new_from_array(pk_bytes);
        let did_hash = state::hash_bytes(merchant_did.as_bytes());

        let slot = ctx.accounts.clock.slot;
        let mut message = Vec::with_capacity(32 + 32 + 32 + 8);
        message.extend_from_slice(&did_hash);
        message.extend_from_slice(active_pubkey.as_ref());
        message.extend_from_slice(&platform_vc_hash);
        message.extend_from_slice(&slot.to_le_bytes());

        require!(
            verify_ed25519_signature(&message, &signature, &did_pubkey),
            DidError::InvalidSignature
        );

        let leaf_hash = compute_merchant_leaf_hash(
            &did_hash, &active_pubkey, &platform_vc_hash,
            state::MERCHANT_STATUS_ACTIVE, slot,
        );

        // CPI: append
        let mut data = Vec::with_capacity(40);
        data.extend_from_slice(&anchor_sighash("append"));
        data.extend_from_slice(&leaf_hash);

        let accounts = vec![
            AccountMeta::new(ctx.accounts.merkle_tree.key(), false),
            AccountMeta::new_readonly(ctx.accounts.tree_authority.key(), true),
            AccountMeta::new_readonly(spl_noop_id(), false),
        ];
        let ix = Instruction { program_id: spl_account_compression_id(), accounts, data };

        let seeds: &[&[u8]] = &[
            b"did-tree-authority",
            ctx.accounts.did_config.merkle_tree.as_ref(),
            &[ctx.bumps.tree_authority],
        ];

        invoke_signed(
            &ix,
            &[
                ctx.accounts.merkle_tree.to_account_info(),
                ctx.accounts.tree_authority.to_account_info(),
            ],
            &[seeds],
        ).map_err(|_| DidError::CpiFailed)?;

        Ok(())
    }

    // ─── 3. rotate_key ───

    #[derive(Accounts)]
    #[instruction(
        merchant_did: String,
        old_active_pubkey: Pubkey,
        new_active_pubkey: Pubkey,
        platform_vc_hash: [u8; 32],
        old_status: u8,
        old_slot: u64,
        leaf_index: u32,
        root: [u8; 32],
        signature: [u8; 64],
    )]
    pub struct RotateKey<'info> {
        #[account(seeds = [b"did-config"], bump = did_config.bump)]
        pub did_config: Account<'info, DidConfig>,
        /// CHECK: Tree authority PDA
        #[account(
            seeds = [b"did-tree-authority", did_config.merkle_tree.as_ref()],
            bump,
        )]
        pub tree_authority: UncheckedAccount<'info>,
        /// CHECK: Merkle tree account
        #[account(mut)]
        pub merkle_tree: UncheckedAccount<'info>,
        #[account(mut)]
        pub payer: Signer<'info>,
        pub clock: Sysvar<'info, Clock>,
        pub system_program: Program<'info, System>,
    }

    pub fn rotate_key<'a>(
        ctx: Context<'a, RotateKey<'a>>,
        merchant_did: String,
        old_active_pubkey: Pubkey,
        new_active_pubkey: Pubkey,
        platform_vc_hash: [u8; 32],
        old_status: u8,
        old_slot: u64,
        leaf_index: u32,
        root: [u8; 32],
        signature: [u8; 64],
    ) -> Result<()> {
        let pk_bytes = extract_pubkey_from_did(&merchant_did)
            .ok_or(DidError::InvalidDidFormat)?;
        let did_pubkey = Pubkey::new_from_array(pk_bytes);
        let did_hash = state::hash_bytes(merchant_did.as_bytes());

        let new_slot = ctx.accounts.clock.slot;
        let mut message = Vec::with_capacity(32 + 32 + 32 + 32 + 8);
        message.extend_from_slice(&did_hash);
        message.extend_from_slice(old_active_pubkey.as_ref());
        message.extend_from_slice(new_active_pubkey.as_ref());
        message.extend_from_slice(&platform_vc_hash);
        message.extend_from_slice(&new_slot.to_le_bytes());

        require!(
            verify_ed25519_signature(&message, &signature, &did_pubkey),
            DidError::InvalidSignature
        );

        let old_leaf = compute_merchant_leaf_hash(
            &did_hash, &old_active_pubkey, &platform_vc_hash, old_status, old_slot,
        );
        let new_leaf = compute_merchant_leaf_hash(
            &did_hash, &new_active_pubkey, &platform_vc_hash, old_status, new_slot,
        );

        // CPI: replace_leaf
        let mut data = Vec::with_capacity(108);
        data.extend_from_slice(&anchor_sighash("replace_leaf"));
        data.extend_from_slice(&root);
        data.extend_from_slice(&old_leaf);
        data.extend_from_slice(&new_leaf);
        data.extend_from_slice(&leaf_index.to_le_bytes());

        let mut accounts = vec![
            AccountMeta::new(ctx.accounts.merkle_tree.key(), false),
            AccountMeta::new_readonly(ctx.accounts.tree_authority.key(), true),
            AccountMeta::new_readonly(spl_noop_id(), false),
        ];
        for acc in ctx.remaining_accounts {
            accounts.push(AccountMeta::new_readonly(acc.key(), false));
        }
        let ix = Instruction { program_id: spl_account_compression_id(), accounts, data };

        let mut all_accounts = vec![
            ctx.accounts.merkle_tree.to_account_info(),
            ctx.accounts.tree_authority.to_account_info(),
        ];
        all_accounts.extend(ctx.remaining_accounts.iter().cloned());

        let seeds: &[&[u8]] = &[
            b"did-tree-authority",
            ctx.accounts.did_config.merkle_tree.as_ref(),
            &[ctx.bumps.tree_authority],
        ];

        invoke_signed(&ix, &all_accounts, &[seeds]).map_err(|_| DidError::CpiFailed)?;
        Ok(())
    }

    // ─── 4. update_vc ───

    #[derive(Accounts)]
    #[instruction(
        merchant_did_hash: [u8; 32],
        active_pubkey: Pubkey,
        new_vc_hash: [u8; 32],
        status: u8,
        old_slot: u64,
        leaf_index: u32,
        root: [u8; 32],
        old_vc_hash: [u8; 32],
    )]
    pub struct UpdateVc<'info> {
        #[account(
            seeds = [b"did-config"],
            bump = did_config.bump,
            constraint = did_config.platform_authority == authority.key() @ DidError::InvalidAuthority,
        )]
        pub did_config: Account<'info, DidConfig>,
        /// CHECK: Tree authority PDA
        #[account(
            seeds = [b"did-tree-authority", did_config.merkle_tree.as_ref()],
            bump,
        )]
        pub tree_authority: UncheckedAccount<'info>,
        /// CHECK: Merkle tree account
        #[account(mut)]
        pub merkle_tree: UncheckedAccount<'info>,
        pub authority: Signer<'info>,
        pub clock: Sysvar<'info, Clock>,
    }

    pub fn update_vc<'a>(
        ctx: Context<'a, UpdateVc<'a>>,
        merchant_did_hash: [u8; 32],
        active_pubkey: Pubkey,
        new_vc_hash: [u8; 32],
        status: u8,
        old_slot: u64,
        leaf_index: u32,
        root: [u8; 32],
        old_vc_hash: [u8; 32],
    ) -> Result<()> {
        let old_leaf = compute_merchant_leaf_hash(
            &merchant_did_hash, &active_pubkey, &old_vc_hash, status, old_slot,
        );
        let new_slot = ctx.accounts.clock.slot;
        let new_leaf = compute_merchant_leaf_hash(
            &merchant_did_hash, &active_pubkey, &new_vc_hash, status, new_slot,
        );

        // CPI: replace_leaf
        let mut data = Vec::with_capacity(108);
        data.extend_from_slice(&anchor_sighash("replace_leaf"));
        data.extend_from_slice(&root);
        data.extend_from_slice(&old_leaf);
        data.extend_from_slice(&new_leaf);
        data.extend_from_slice(&leaf_index.to_le_bytes());

        let mut accounts = vec![
            AccountMeta::new(ctx.accounts.merkle_tree.key(), false),
            AccountMeta::new_readonly(ctx.accounts.tree_authority.key(), true),
            AccountMeta::new_readonly(spl_noop_id(), false),
        ];
        for acc in ctx.remaining_accounts {
            accounts.push(AccountMeta::new_readonly(acc.key(), false));
        }
        let ix = Instruction { program_id: spl_account_compression_id(), accounts, data };

        let mut all_accounts = vec![
            ctx.accounts.merkle_tree.to_account_info(),
            ctx.accounts.tree_authority.to_account_info(),
        ];
        all_accounts.extend(ctx.remaining_accounts.iter().cloned());

        let seeds: &[&[u8]] = &[
            b"did-tree-authority",
            ctx.accounts.did_config.merkle_tree.as_ref(),
            &[ctx.bumps.tree_authority],
        ];

        invoke_signed(&ix, &all_accounts, &[seeds]).map_err(|_| DidError::CpiFailed)?;
        Ok(())
    }

    // ─── 5. update_status ───

    #[derive(Accounts)]
    #[instruction(
        merchant_did_hash: [u8; 32],
        active_pubkey: Pubkey,
        platform_vc_hash: [u8; 32],
        old_status: u8,
        new_status: u8,
        old_slot: u64,
        leaf_index: u32,
        root: [u8; 32],
    )]
    pub struct UpdateStatus<'info> {
        #[account(
            seeds = [b"did-config"],
            bump = did_config.bump,
            constraint = did_config.platform_authority == authority.key() @ DidError::InvalidAuthority,
        )]
        pub did_config: Account<'info, DidConfig>,
        /// CHECK: Tree authority PDA
        #[account(
            seeds = [b"did-tree-authority", did_config.merkle_tree.as_ref()],
            bump,
        )]
        pub tree_authority: UncheckedAccount<'info>,
        /// CHECK: Merkle tree account
        #[account(mut)]
        pub merkle_tree: UncheckedAccount<'info>,
        pub authority: Signer<'info>,
        pub clock: Sysvar<'info, Clock>,
    }

    pub fn update_status<'a>(
        ctx: Context<'a, UpdateStatus<'a>>,
        merchant_did_hash: [u8; 32],
        active_pubkey: Pubkey,
        platform_vc_hash: [u8; 32],
        old_status: u8,
        new_status: u8,
        old_slot: u64,
        leaf_index: u32,
        root: [u8; 32],
    ) -> Result<()> {
        require!(
            new_status == state::MERCHANT_STATUS_ACTIVE
                || new_status == state::MERCHANT_STATUS_SUSPENDED
                || new_status == state::MERCHANT_STATUS_REVOKED,
            DidError::InvalidStatus
        );

        let old_leaf = compute_merchant_leaf_hash(
            &merchant_did_hash, &active_pubkey, &platform_vc_hash, old_status, old_slot,
        );
        let new_slot = ctx.accounts.clock.slot;
        let new_leaf = compute_merchant_leaf_hash(
            &merchant_did_hash, &active_pubkey, &platform_vc_hash, new_status, new_slot,
        );

        // CPI: replace_leaf
        let mut data = Vec::with_capacity(108);
        data.extend_from_slice(&anchor_sighash("replace_leaf"));
        data.extend_from_slice(&root);
        data.extend_from_slice(&old_leaf);
        data.extend_from_slice(&new_leaf);
        data.extend_from_slice(&leaf_index.to_le_bytes());

        let mut accounts = vec![
            AccountMeta::new(ctx.accounts.merkle_tree.key(), false),
            AccountMeta::new_readonly(ctx.accounts.tree_authority.key(), true),
            AccountMeta::new_readonly(spl_noop_id(), false),
        ];
        for acc in ctx.remaining_accounts {
            accounts.push(AccountMeta::new_readonly(acc.key(), false));
        }
        let ix = Instruction { program_id: spl_account_compression_id(), accounts, data };

        let mut all_accounts = vec![
            ctx.accounts.merkle_tree.to_account_info(),
            ctx.accounts.tree_authority.to_account_info(),
        ];
        all_accounts.extend(ctx.remaining_accounts.iter().cloned());

        let seeds: &[&[u8]] = &[
            b"did-tree-authority",
            ctx.accounts.did_config.merkle_tree.as_ref(),
            &[ctx.bumps.tree_authority],
        ];

        invoke_signed(&ix, &all_accounts, &[seeds]).map_err(|_| DidError::CpiFailed)?;
        Ok(())
    }

    // ─── 6. verify_merchant ───

    #[derive(Accounts)]
    #[instruction(
        merchant_did_hash: [u8; 32],
        active_pubkey: Pubkey,
        platform_vc_hash: [u8; 32],
        status: u8,
        slot_updated: u64,
        leaf_index: u32,
        root: [u8; 32],
    )]
    pub struct VerifyMerchant<'info> {
        #[account(seeds = [b"did-config"], bump = did_config.bump)]
        pub did_config: Account<'info, DidConfig>,
    }

    pub fn verify_merchant(
        ctx: Context<VerifyMerchant>,
        merchant_did_hash: [u8; 32],
        active_pubkey: Pubkey,
        platform_vc_hash: [u8; 32],
        status: u8,
        slot_updated: u64,
        _leaf_index: u32,
        root: [u8; 32],
    ) -> Result<bool> {
        let leaf_hash = compute_merchant_leaf_hash(
            &merchant_did_hash, &active_pubkey, &platform_vc_hash, status, slot_updated,
        );

        let proof: Vec<[u8; 32]> = ctx
            .remaining_accounts
            .iter()
            .map(|acc| {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(acc.key.as_ref());
                arr
            })
            .collect();

        if !verify_merkle_proof(&leaf_hash, &proof, &root) {
            return Err(DidError::ProofVerificationFailed.into());
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_merchant_leaf_hash_deterministic() {
        let did_hash = [1u8; 32];
        let pubkey = Pubkey::new_unique();
        let vc_hash = [2u8; 32];
        let h1 = compute_merchant_leaf_hash(&did_hash, &pubkey, &vc_hash, 0, 100);
        let h2 = compute_merchant_leaf_hash(&did_hash, &pubkey, &vc_hash, 0, 100);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_compute_merchant_leaf_hash_different_inputs() {
        let h1 = compute_merchant_leaf_hash(&[1u8; 32], &Pubkey::new_unique(), &[2u8; 32], 0, 100);
        let h2 = compute_merchant_leaf_hash(&[3u8; 32], &Pubkey::new_unique(), &[4u8; 32], 1, 200);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_verify_merkle_proof_single_leaf() {
        let leaf_hash = [5u8; 32];
        let proof: Vec<[u8; 32]> = vec![];
        let root = leaf_hash;
        assert!(verify_merkle_proof(&leaf_hash, &proof, &root));
    }

    #[test]
    fn test_verify_merkle_proof_two_leaves() {
        let leaf1 = [1u8; 32];
        let leaf2 = [2u8; 32];
        let root = state::hash_pair(
            &std::cmp::min(leaf1, leaf2),
            &std::cmp::max(leaf1, leaf2),
        );
        assert!(verify_merkle_proof(&leaf1, &[leaf2], &root));
        assert!(verify_merkle_proof(&leaf2, &[leaf1], &root));
    }

    #[test]
    fn test_verify_merkle_proof_fails_wrong_root() {
        let leaf = [1u8; 32];
        let sibling = [2u8; 32];
        let wrong_root = [99u8; 32];
        assert!(!verify_merkle_proof(&leaf, &[sibling], &wrong_root));
    }

    #[test]
    fn test_anchor_sighash_deterministic() {
        let h1 = anchor_sighash("append");
        let h2 = anchor_sighash("append");
        assert_eq!(h1, h2);
        let h3 = anchor_sighash("replace_leaf");
        assert_ne!(h1, h3);
    }
}
