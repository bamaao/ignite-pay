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
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use crate::state::{ChannelAccount, ChannelStatus};
use crate::error::ChannelError;
use crate::utils::merkle::verify_merkle_proof;
use crate::utils::ed25519::verify_ed25519_signature;

#[derive(Accounts)]
pub struct Claim<'info> {
    #[account(
        mut,
        constraint = channel.status == ChannelStatus::Settling @ ChannelError::InvalidStatus,
    )]
    pub channel: Account<'info, ChannelAccount>,

    /// CHECK: Must be user_pubkey or provider_pubkey
    #[account(
        constraint = claimer.key() == channel.user_pubkey
            || claimer.key() == channel.provider_pubkey
            @ ChannelError::Unauthorized,
    )]
    pub claimer: Signer<'info>,

    /// Claimer's vault (SPL token account) to receive claimed funds.
    #[account(mut)]
    pub vault: Account<'info, TokenAccount>,

    /// Channel's escrow vault holding deposited tokens.
    /// CHECK: PDA escrow vault - verified by seeds
    #[account(
        mut,
        seeds = [b"escrow", channel.channel_id.as_ref()],
        bump,
    )]
    pub escrow_vault: AccountInfo<'info>,

    pub token_program: Program<'info, Token>,
    pub clock: Sysvar<'info, Clock>,
}

/// Claim a leaf's funds during settlement.
///
/// Validates: settle_deadline, Merkle proof, leaf ownership, amount, signature.
/// BUG-20 fix: accepts `leaf_data` (borsh-serialized UTXOLeaf), hashes it on-chain
/// and verifies it matches `leaf_hash`, then extracts the amount to ensure
/// `claim_amount` is committed by the leaf hash.
pub fn claim(
    ctx: Context<Claim>,
    leaf_index: u32,
    claim_amount: u64,
    leaf_owner: Pubkey,
    leaf_hash: [u8; 32],
    proof: Vec<[u8; 32]>,
    leaf_data: Vec<u8>,
    claimer_signature: [u8; 64],
) -> Result<()> {
    let channel = &mut ctx.accounts.channel;
    let current_slot = ctx.accounts.clock.slot;

    // Check settle_deadline
    let deadline = channel.settle_deadline
        .ok_or(ChannelError::InvalidStatus)?;
    require!(
        current_slot <= deadline,
        ChannelError::SettlementExpired
    );

    // Verify claimer owns the leaf
    require!(
        claimer.key() == leaf_owner,
        ChannelError::InvalidOwner
    );

    // Duplicate claim prevention
    require!(
        !channel.claimed_leaves.contains(&leaf_index),
        ChannelError::AlreadyClaimed
    );

    // BUG-20 fix: verify leaf_data hashes to leaf_hash, then extract amount.
    // UTXOLeaf borsh layout: u8(leaf_type) + 32 bytes(owner Pubkey) + u64(amount) + ...
    // The amount field starts at byte offset 33 (1 + 32).
    {
        let computed_hash = anchor_lang::solana_program::hash::hash(&leaf_data).to_bytes();
        require!(
            computed_hash == leaf_hash,
            ChannelError::InvalidLeafData
        );

        // Extract amount from leaf_data (offset 33, 8 bytes LE)
        require!(
            leaf_data.len() >= 41,
            ChannelError::InvalidLeafData
        );
        let amount_bytes: [u8; 8] = leaf_data[33..41]
            .try_into()
            .map_err(|_| ChannelError::InvalidLeafData)?;
        let extracted_amount = u64::from_le_bytes(amount_bytes);
        require!(
            extracted_amount == claim_amount,
            ChannelError::AmountMismatch
        );
    }

    // Verify Merkle proof
    require!(
        verify_merkle_proof(&leaf_hash, &proof, &channel.current_root),
        ChannelError::ProofVerificationFailed
    );

    // PROG-8 fix: verify claimer's signature on (channel_id, current_slot, root)
    let mut claim_msg = Vec::with_capacity(32 + 8 + 32);
    claim_msg.extend_from_slice(&channel.channel_id);
    claim_msg.extend_from_slice(&current_slot.to_le_bytes());
    claim_msg.extend_from_slice(&channel.current_root);
    require!(
        verify_ed25519_signature(&claim_msg, &claimer_signature, &ctx.accounts.claimer.key()),
        ChannelError::InvalidSignature
    );

    // Update total_claimed with overflow protection
    channel.total_claimed = channel.total_claimed
        .checked_add(claim_amount)
        .ok_or(ChannelError::ArithmeticOverflow)?;

    require!(
        channel.total_claimed <= channel.total_deposited,
        ChannelError::AmountConservation
    );

    // Mark leaf as claimed
    channel.claimed_leaves.push(leaf_index);

    // BUG-35 / PROG-13 fix: SPL Token CPI transfer from escrow to claimer's vault
    if claim_amount > 0 {
        let bump = ctx.bumps.escrow_vault;
        let seeds: &[&[u8]] = &[b"escrow", channel.channel_id.as_ref(), &[bump]];

        let cpi_accounts = Transfer {
            from: ctx.accounts.escrow_vault.to_account_info(),
            to: ctx.accounts.vault.to_account_info(),
            authority: ctx.accounts.escrow_vault.to_account_info(),
        };
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts,
                &[seeds],
            ),
            claim_amount,
        )?;
    }

    Ok(())
}
