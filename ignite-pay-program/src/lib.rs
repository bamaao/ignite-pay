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

#![allow(unexpected_cfgs)]
pub mod state;
pub mod error;
pub mod utils;

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use crate::state::{ChannelAccount, ChannelStatus};
use crate::error::ChannelError;
use crate::utils::merkle::verify_merkle_proof;

declare_id!("DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe");

const LEAF_TYPE_HTLC: u8 = 1;

#[program]
pub mod ignite_pay_program {
    use super::*;

    // ── 1. OPEN CHANNEL ──

    #[derive(Accounts)]
    #[instruction(
        channel_id: [u8; 32],
        deposit_a: u64,
        tree_depth: u32,
        open_slot: u64,
        challenge_duration: u64,
        min_challenge_delay: u64,
        initial_root: [u8; 32],
    )]
    pub struct OpenChannel<'info> {
        #[account(
            init,
            payer = payer,
            space = ChannelAccount::space(tree_depth),
            seeds = [b"channel", channel_id.as_ref()],
            bump,
        )]
        pub channel: Account<'info, ChannelAccount>,

        #[account(
            constraint = user.key() == user_pubkey.key() @ ChannelError::Unauthorized,
        )]
        pub user: Signer<'info>,

        /// CHECK: User pubkey
        pub user_pubkey: UncheckedAccount<'info>,

        /// CHECK: Provider pubkey
        pub provider_pubkey: UncheckedAccount<'info>,

        /// CHECK: Token mint
        pub token_mint: UncheckedAccount<'info>,

        #[account(
            constraint = vault_a.mint == token_mint.key() @ ChannelError::InvalidOwner,
        )]
        pub vault_a: Account<'info, TokenAccount>,

        #[account(
            constraint = vault_b.mint == token_mint.key() @ ChannelError::InvalidOwner,
        )]
        pub vault_b: Account<'info, TokenAccount>,

        #[account(mut)]
        pub payer: Signer<'info>,

        pub system_program: Program<'info, System>,
        pub token_program: Program<'info, Token>,
        pub rent: Sysvar<'info, Rent>,

        /// CHECK: Instruction sysvar for ed25519 introspection
        pub instruction_sysvar: UncheckedAccount<'info>,
    }

    pub fn open_channel(
        ctx: Context<OpenChannel>,
        channel_id: [u8; 32],
        deposit_a: u64,
        tree_depth: u32,
        open_slot: u64,
        challenge_duration: u64,
        min_challenge_delay: u64,
        initial_root: [u8; 32],
    ) -> Result<()> {
        require!(tree_depth <= 12, ChannelError::LeafIndexOutOfBounds);
        require!(deposit_a > 0, ChannelError::ZeroDeposit);

        let channel = &mut ctx.accounts.channel;
        channel.channel_id = channel_id;
        channel.user_pubkey = ctx.accounts.user.key();
        channel.provider_pubkey = ctx.accounts.provider_pubkey.key();
        channel.token_mint = ctx.accounts.token_mint.key();
        channel.status = ChannelStatus::Open;
        channel.sequence = 0;
        channel.current_root = initial_root;
        channel.total_deposited = deposit_a;
        channel.open_slot = open_slot;
        channel.challenge_slot = None;
        channel.vault_a = ctx.accounts.vault_a.key();
        channel.vault_b = ctx.accounts.vault_b.key();
        channel.deposit_a = deposit_a;
        channel.deposit_b = 0;
        channel.challenge_duration = challenge_duration;
        channel.min_challenge_delay = min_challenge_delay;
        channel.total_claimed = 0;
        channel.settle_deadline = None;
        channel.tree_depth = tree_depth;
        channel.leaf_count = 1;
        channel.claimed_leaves = Vec::new();
        channel.auto_close_slot = None;

        let mut msg = Vec::with_capacity(32 + 8 + 4 + 32);
        msg.extend_from_slice(&channel_id);
        msg.extend_from_slice(&deposit_a.to_le_bytes());
        msg.extend_from_slice(&tree_depth.to_le_bytes());
        msg.extend_from_slice(&initial_root);

        let current_ix = read_current_ix_index(&ctx.accounts.instruction_sysvar)?;
        verify_ed25519_signature_introspection(
            &ctx.accounts.instruction_sysvar,
            &channel.user_pubkey,
            &msg,
            current_ix,
        )?;

        Ok(())
    }

    // ── 2. FUND CHANNEL ──

    #[derive(Accounts)]
    pub struct FundChannel<'info> {
        #[account(
            mut,
            constraint = channel.status == ChannelStatus::Open @ ChannelError::InvalidStatus,
            constraint = channel.deposit_b == 0 @ ChannelError::AlreadyFunded,
        )]
        pub channel: Account<'info, ChannelAccount>,

        #[account(
            constraint = signer.key() == channel.provider_pubkey @ ChannelError::Unauthorized,
        )]
        pub signer: Signer<'info>,

        #[account(mut)]
        pub source_vault: Account<'info, TokenAccount>,

        #[account(
            mut,
            constraint = vault_b.key() == channel.vault_b @ ChannelError::InvalidOwner,
        )]
        pub vault_b: Account<'info, TokenAccount>,

        pub token_program: Program<'info, Token>,
    }

    pub fn fund_channel(ctx: Context<FundChannel>, deposit_b: u64) -> Result<()> {
        require!(deposit_b > 0, ChannelError::ZeroDeposit);

        let cpi_accounts = Transfer {
            from: ctx.accounts.source_vault.to_account_info(),
            to: ctx.accounts.vault_b.to_account_info(),
            authority: ctx.accounts.signer.to_account_info(),
        };
        token::transfer(CpiContext::new(ctx.accounts.token_program.key(), cpi_accounts), deposit_b)?;

        let channel = &mut ctx.accounts.channel;
        channel.deposit_b = deposit_b;
        channel.total_deposited = channel.total_deposited
            .checked_add(deposit_b)
            .ok_or(ChannelError::ArithmeticOverflow)?;
        channel.leaf_count = channel.leaf_count
            .checked_add(1)
            .ok_or(ChannelError::ArithmeticOverflow)?;

        Ok(())
    }

    // ── 3. COOPERATIVE SETTLE ──

    #[derive(Accounts)]
    pub struct CooperativeSettle<'info> {
        #[account(
            mut,
            constraint = channel.status == ChannelStatus::Open @ ChannelError::InvalidStatus,
        )]
        pub channel: Account<'info, ChannelAccount>,

        pub clock: Sysvar<'info, Clock>,

        /// CHECK: Instruction sysvar for ed25519 introspection
        pub instruction_sysvar: UncheckedAccount<'info>,
    }

    pub fn cooperative_settle(
        ctx: Context<CooperativeSettle>,
        sequence: u64,
        root: [u8; 32],
        settle_window: u64,
    ) -> Result<()> {
        let channel = &mut ctx.accounts.channel;
        let current_slot = ctx.accounts.clock.slot;

        require!(sequence >= channel.sequence, ChannelError::InvalidSequence);
        require!(root == channel.current_root, ChannelError::PrevHashMismatch);

        let mut msg = Vec::with_capacity(32 + 8 + 32);
        msg.extend_from_slice(&channel.channel_id);
        msg.extend_from_slice(&sequence.to_le_bytes());
        msg.extend_from_slice(&root);

        let current_ix = read_current_ix_index(&ctx.accounts.instruction_sysvar)?;
        // Two signatures: sig_a (user) and sig_b (provider)
        // Both must be verified via preceding ed25519 instructions
        verify_ed25519_signature_introspection(
            &ctx.accounts.instruction_sysvar,
            &channel.user_pubkey,
            &msg,
            current_ix,
        )?;
        verify_ed25519_signature_introspection(
            &ctx.accounts.instruction_sysvar,
            &channel.provider_pubkey,
            &msg,
            current_ix,
        )?;

        channel.status = ChannelStatus::Settling;
        channel.settle_deadline = Some(current_slot + settle_window);

        Ok(())
    }

    // ── 4. TRIGGER CHALLENGE ──

    #[derive(Accounts)]
    pub struct TriggerChallenge<'info> {
        #[account(
            mut,
            constraint = channel.status == ChannelStatus::Open @ ChannelError::InvalidStatus,
        )]
        pub channel: Account<'info, ChannelAccount>,

        #[account(
            constraint = challenger.key() == channel.user_pubkey
                || challenger.key() == channel.provider_pubkey
                @ ChannelError::Unauthorized,
        )]
        pub challenger: Signer<'info>,

        pub clock: Sysvar<'info, Clock>,

        /// CHECK: Instruction sysvar for ed25519 introspection
        pub instruction_sysvar: UncheckedAccount<'info>,
    }

    pub fn trigger_challenge(
        ctx: Context<TriggerChallenge>,
        submitted_root: [u8; 32],
        submitted_sequence: u64,
    ) -> Result<()> {
        let channel = &mut ctx.accounts.channel;
        let current_slot = ctx.accounts.clock.slot;

        let min_slot = channel.open_slot + channel.min_challenge_delay;
        require!(current_slot >= min_slot, ChannelError::ChallengeNotElapsed);
        require!(submitted_sequence > channel.sequence, ChannelError::InvalidSequence);

        let mut msg = Vec::with_capacity(32 + 8 + 32);
        msg.extend_from_slice(&channel.channel_id);
        msg.extend_from_slice(&current_slot.to_le_bytes());
        msg.extend_from_slice(&submitted_root);

        let current_ix = read_current_ix_index(&ctx.accounts.instruction_sysvar)?;
        verify_ed25519_signature_introspection(
            &ctx.accounts.instruction_sysvar,
            &ctx.accounts.challenger.key(),
            &msg,
            current_ix,
        )?;

        channel.status = ChannelStatus::Challenged;
        channel.challenge_slot = Some(current_slot);
        channel.current_root = submitted_root;
        channel.sequence = submitted_sequence;

        Ok(())
    }

    // ── 5. SUBMIT COUNTER STATE ──

    #[derive(Accounts)]
    pub struct SubmitCounterState<'info> {
        #[account(
            mut,
            constraint = channel.status == ChannelStatus::Challenged @ ChannelError::InvalidStatus,
        )]
        pub channel: Account<'info, ChannelAccount>,

        /// CHECK: Instruction sysvar for ed25519 introspection
        pub instruction_sysvar: UncheckedAccount<'info>,
    }

    pub fn submit_counter_state(
        ctx: Context<SubmitCounterState>,
        sequence: u64,
        root: [u8; 32],
    ) -> Result<()> {
        let channel = &mut ctx.accounts.channel;
        require!(sequence > channel.sequence, ChannelError::InvalidSequence);

        let mut msg = Vec::with_capacity(32 + 8 + 32);
        msg.extend_from_slice(&channel.channel_id);
        msg.extend_from_slice(&sequence.to_le_bytes());
        msg.extend_from_slice(&root);

        let current_ix = read_current_ix_index(&ctx.accounts.instruction_sysvar)?;
        verify_ed25519_signature_introspection(
            &ctx.accounts.instruction_sysvar,
            &channel.user_pubkey,
            &msg,
            current_ix,
        )?;
        verify_ed25519_signature_introspection(
            &ctx.accounts.instruction_sysvar,
            &channel.provider_pubkey,
            &msg,
            current_ix,
        )?;

        channel.sequence = sequence;
        channel.current_root = root;

        Ok(())
    }

    // ── 6. SETTLE AFTER TIMEOUT ──

    #[derive(Accounts)]
    pub struct SettleAfterTimeout<'info> {
        #[account(
            mut,
            constraint = channel.status == ChannelStatus::Challenged
                || (channel.status == ChannelStatus::Open
                    && channel.auto_close_slot.is_some())
                @ ChannelError::InvalidStatus,
        )]
        pub channel: Account<'info, ChannelAccount>,

        pub clock: Sysvar<'info, Clock>,
    }

    pub fn settle_after_timeout(
        ctx: Context<SettleAfterTimeout>,
        settle_window: u64,
    ) -> Result<()> {
        let channel = &mut ctx.accounts.channel;
        let current_slot = ctx.accounts.clock.slot;

        if channel.status == ChannelStatus::Open {
            let auto_close = channel.auto_close_slot
                .ok_or(ChannelError::InvalidStatus)?;
            require!(current_slot >= auto_close, ChannelError::ChallengeNotElapsed);
        } else {
            let challenge_slot = channel.challenge_slot
                .ok_or(ChannelError::InvalidStatus)?;
            require!(
                current_slot > challenge_slot + channel.challenge_duration,
                ChannelError::ChallengeNotElapsed
            );
        }

        channel.status = ChannelStatus::Settling;
        channel.settle_deadline = Some(current_slot + settle_window);

        Ok(())
    }

    // ── 7. CLAIM ──

    #[derive(Accounts)]
    pub struct Claim<'info> {
        #[account(
            mut,
            constraint = channel.status == ChannelStatus::Settling @ ChannelError::InvalidStatus,
        )]
        pub channel: Account<'info, ChannelAccount>,

        #[account(
            constraint = claimer.key() == channel.user_pubkey
                || claimer.key() == channel.provider_pubkey
                @ ChannelError::Unauthorized,
        )]
        pub claimer: Signer<'info>,

        #[account(mut)]
        pub vault: Account<'info, TokenAccount>,

        /// CHECK: PDA escrow vault
        #[account(
            mut,
            seeds = [b"escrow", channel.channel_id.as_ref()],
            bump,
        )]
        pub escrow_vault: UncheckedAccount<'info>,

        pub token_program: Program<'info, Token>,
        pub clock: Sysvar<'info, Clock>,

        /// CHECK: Instruction sysvar for ed25519 introspection
        pub instruction_sysvar: UncheckedAccount<'info>,
    }

    pub fn claim(
        ctx: Context<Claim>,
        leaf_index: u32,
        claim_amount: u64,
        leaf_owner: Pubkey,
        leaf_hash: [u8; 32],
        proof: Vec<[u8; 32]>,
        leaf_data: Vec<u8>,
    ) -> Result<()> {
        let channel = &mut ctx.accounts.channel;
        let current_slot = ctx.accounts.clock.slot;

        let deadline = channel.settle_deadline
            .ok_or(ChannelError::InvalidStatus)?;
        require!(current_slot <= deadline, ChannelError::SettlementExpired);
        require!(ctx.accounts.claimer.key() == leaf_owner, ChannelError::InvalidOwner);
        require!(
            !channel.claimed_leaves.contains(&leaf_index),
            ChannelError::AlreadyClaimed
        );

        {
            let computed_hash = solana_program::hash::hash(&leaf_data).to_bytes();
            require!(computed_hash == leaf_hash, ChannelError::InvalidLeafData);
            require!(leaf_data.len() >= 41, ChannelError::InvalidLeafData);
            let amount_bytes: [u8; 8] = leaf_data[33..41]
                .try_into()
                .map_err(|_| ChannelError::InvalidLeafData)?;
            let extracted_amount = u64::from_le_bytes(amount_bytes);
            require!(extracted_amount == claim_amount, ChannelError::AmountMismatch);
        }

        require!(
            verify_merkle_proof(&leaf_hash, &proof, &channel.current_root),
            ChannelError::ProofVerificationFailed
        );

        let mut claim_msg = Vec::with_capacity(32 + 8 + 32);
        claim_msg.extend_from_slice(&channel.channel_id);
        claim_msg.extend_from_slice(&current_slot.to_le_bytes());
        claim_msg.extend_from_slice(&channel.current_root);

        let current_ix = read_current_ix_index(&ctx.accounts.instruction_sysvar)?;
        verify_ed25519_signature_introspection(
            &ctx.accounts.instruction_sysvar,
            &ctx.accounts.claimer.key(),
            &claim_msg,
            current_ix,
        )?;

        channel.total_claimed = channel.total_claimed
            .checked_add(claim_amount)
            .ok_or(ChannelError::ArithmeticOverflow)?;
        require!(
            channel.total_claimed <= channel.total_deposited,
            ChannelError::AmountConservation
        );
        channel.claimed_leaves.push(leaf_index);

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
                    ctx.accounts.token_program.key(),
                    cpi_accounts,
                    &[seeds],
                ),
                claim_amount,
            )?;
        }

        Ok(())
    }

    // ── 8. VERIFY HTLC ──

    #[derive(Accounts)]
    pub struct VerifyHtlc<'info> {
        #[account(
            mut,
            constraint = channel.status == ChannelStatus::Challenged
                || channel.status == ChannelStatus::Settling
                @ ChannelError::InvalidStatus,
        )]
        pub channel: Account<'info, ChannelAccount>,

        #[account(
            constraint = claimer.key() == channel.user_pubkey
                || claimer.key() == channel.provider_pubkey
                @ ChannelError::Unauthorized,
        )]
        pub claimer: Signer<'info>,

        #[account(mut)]
        pub vault: Account<'info, TokenAccount>,

        /// CHECK: PDA escrow vault
        #[account(
            mut,
            seeds = [b"escrow", channel.channel_id.as_ref()],
            bump,
        )]
        pub escrow_vault: UncheckedAccount<'info>,

        pub token_program: Program<'info, Token>,
        pub clock: Sysvar<'info, Clock>,

        /// CHECK: Instruction sysvar for ed25519 introspection
        pub instruction_sysvar: UncheckedAccount<'info>,
    }

    pub fn verify_htlc(
        ctx: Context<VerifyHtlc>,
        leaf_index: u32,
        preimage: [u8; 32],
        hash_lock: [u8; 32],
        leaf_amount: u64,
        beneficiary: Pubkey,
        leaf_hash: [u8; 32],
        proof: Vec<[u8; 32]>,
        timelock_slot: u64,
        leaf_data: Vec<u8>,
    ) -> Result<()> {
        let channel = &mut ctx.accounts.channel;
        let current_slot = ctx.accounts.clock.slot;

        let deadline = if channel.status == ChannelStatus::Challenged {
            let cs = channel.challenge_slot
                .ok_or(ChannelError::InvalidStatus)?;
            cs.checked_add(channel.challenge_duration)
                .ok_or(ChannelError::ArithmeticOverflow)?
        } else {
            channel.settle_deadline
                .ok_or(ChannelError::InvalidStatus)?
        };
        require!(current_slot <= deadline, ChannelError::SettlementExpired);
        require!(ctx.accounts.claimer.key() == beneficiary, ChannelError::InvalidOwner);
        require!(
            !channel.claimed_leaves.contains(&leaf_index),
            ChannelError::AlreadyClaimed
        );

        {
            let computed_hash = solana_program::hash::hash(&leaf_data).to_bytes();
            require!(computed_hash == leaf_hash, ChannelError::InvalidLeafData);
            require!(leaf_data.len() >= 41, ChannelError::InvalidLeafData);
            let leaf_type = leaf_data[0];
            require!(leaf_type == LEAF_TYPE_HTLC, ChannelError::InvalidLeafData);
            let amount_bytes: [u8; 8] = leaf_data[33..41]
                .try_into()
                .map_err(|_| ChannelError::InvalidLeafData)?;
            let extracted_amount = u64::from_le_bytes(amount_bytes);
            require!(extracted_amount == leaf_amount, ChannelError::AmountMismatch);
            let beneficiary_bytes: [u8; 32] = leaf_data[1..33]
                .try_into()
                .map_err(|_| ChannelError::InvalidLeafData)?;
            let extracted_beneficiary = Pubkey::new_from_array(beneficiary_bytes);
            require!(extracted_beneficiary == beneficiary, ChannelError::InvalidLeafData);
        }

        let computed_hash = solana_program::hash::hash(&preimage).to_bytes();
        require!(computed_hash == hash_lock, ChannelError::HashLockMismatch);
        require!(current_slot <= timelock_slot, ChannelError::HtlcExpired);

        require!(
            verify_merkle_proof(&leaf_hash, &proof, &channel.current_root),
            ChannelError::ProofVerificationFailed
        );

        let mut htlc_msg = Vec::with_capacity(32 + 8 + 32);
        htlc_msg.extend_from_slice(&channel.channel_id);
        htlc_msg.extend_from_slice(&current_slot.to_le_bytes());
        htlc_msg.extend_from_slice(&channel.current_root);

        let current_ix = read_current_ix_index(&ctx.accounts.instruction_sysvar)?;
        verify_ed25519_signature_introspection(
            &ctx.accounts.instruction_sysvar,
            &ctx.accounts.claimer.key(),
            &htlc_msg,
            current_ix,
        )?;

        channel.total_claimed = channel.total_claimed
            .checked_add(leaf_amount)
            .ok_or(ChannelError::ArithmeticOverflow)?;
        require!(
            channel.total_claimed <= channel.total_deposited,
            ChannelError::AmountConservation
        );
        channel.claimed_leaves.push(leaf_index);

        if leaf_amount > 0 {
            let bump = ctx.bumps.escrow_vault;
            let seeds: &[&[u8]] = &[b"escrow", channel.channel_id.as_ref(), &[bump]];
            let cpi_accounts = Transfer {
                from: ctx.accounts.escrow_vault.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
                authority: ctx.accounts.escrow_vault.to_account_info(),
            };
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.key(),
                    cpi_accounts,
                    &[seeds],
                ),
                leaf_amount,
            )?;
        }

        Ok(())
    }

    // ── 9. HTLC REFUND ──

    #[derive(Accounts)]
    pub struct HtlcRefund<'info> {
        #[account(
            mut,
            constraint = channel.status == ChannelStatus::Challenged
                || channel.status == ChannelStatus::Settling
                @ ChannelError::InvalidStatus,
        )]
        pub channel: Account<'info, ChannelAccount>,

        #[account(
            constraint = claimer.key() == channel.user_pubkey
                || claimer.key() == channel.provider_pubkey
                @ ChannelError::Unauthorized,
        )]
        pub claimer: Signer<'info>,

        #[account(mut)]
        pub vault: Account<'info, TokenAccount>,

        /// CHECK: PDA escrow vault
        #[account(
            mut,
            seeds = [b"escrow", channel.channel_id.as_ref()],
            bump,
        )]
        pub escrow_vault: UncheckedAccount<'info>,

        pub token_program: Program<'info, Token>,
        pub clock: Sysvar<'info, Clock>,

        /// CHECK: Instruction sysvar for ed25519 introspection
        pub instruction_sysvar: UncheckedAccount<'info>,
    }

    pub fn htlc_refund(
        ctx: Context<HtlcRefund>,
        leaf_index: u32,
        timelock_slot: u64,
        leaf_amount: u64,
        leaf_owner: Pubkey,
        leaf_hash: [u8; 32],
        proof: Vec<[u8; 32]>,
        leaf_data: Vec<u8>,
    ) -> Result<()> {
        let channel = &mut ctx.accounts.channel;
        let current_slot = ctx.accounts.clock.slot;

        let deadline = if channel.status == ChannelStatus::Challenged {
            let cs = channel.challenge_slot
                .ok_or(ChannelError::InvalidStatus)?;
            cs.checked_add(channel.challenge_duration)
                .ok_or(ChannelError::ArithmeticOverflow)?
        } else {
            channel.settle_deadline
                .ok_or(ChannelError::InvalidStatus)?
        };
        require!(current_slot <= deadline, ChannelError::SettlementExpired);
        require!(ctx.accounts.claimer.key() == leaf_owner, ChannelError::InvalidOwner);
        require!(
            !channel.claimed_leaves.contains(&leaf_index),
            ChannelError::AlreadyClaimed
        );

        {
            let computed_hash = solana_program::hash::hash(&leaf_data).to_bytes();
            require!(computed_hash == leaf_hash, ChannelError::InvalidLeafData);
            require!(leaf_data.len() >= 41, ChannelError::InvalidLeafData);
            let leaf_type = leaf_data[0];
            require!(leaf_type == LEAF_TYPE_HTLC, ChannelError::InvalidLeafData);
            let amount_bytes: [u8; 8] = leaf_data[33..41]
                .try_into()
                .map_err(|_| ChannelError::InvalidLeafData)?;
            let extracted_amount = u64::from_le_bytes(amount_bytes);
            require!(extracted_amount == leaf_amount, ChannelError::AmountMismatch);
            let owner_bytes: [u8; 32] = leaf_data[1..33]
                .try_into()
                .map_err(|_| ChannelError::InvalidLeafData)?;
            let extracted_owner = Pubkey::new_from_array(owner_bytes);
            require!(extracted_owner == leaf_owner, ChannelError::InvalidLeafData);
        }

        require!(current_slot > timelock_slot, ChannelError::HtlcNotExpired);

        require!(
            verify_merkle_proof(&leaf_hash, &proof, &channel.current_root),
            ChannelError::ProofVerificationFailed
        );

        let mut refund_msg = Vec::with_capacity(32 + 8 + 32);
        refund_msg.extend_from_slice(&channel.channel_id);
        refund_msg.extend_from_slice(&current_slot.to_le_bytes());
        refund_msg.extend_from_slice(&channel.current_root);

        let current_ix = read_current_ix_index(&ctx.accounts.instruction_sysvar)?;
        verify_ed25519_signature_introspection(
            &ctx.accounts.instruction_sysvar,
            &ctx.accounts.claimer.key(),
            &refund_msg,
            current_ix,
        )?;

        channel.total_claimed = channel.total_claimed
            .checked_add(leaf_amount)
            .ok_or(ChannelError::ArithmeticOverflow)?;
        require!(
            channel.total_claimed <= channel.total_deposited,
            ChannelError::AmountConservation
        );
        channel.claimed_leaves.push(leaf_index);

        if leaf_amount > 0 {
            let bump = ctx.bumps.escrow_vault;
            let seeds: &[&[u8]] = &[b"escrow", channel.channel_id.as_ref(), &[bump]];
            let cpi_accounts = Transfer {
                from: ctx.accounts.escrow_vault.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
                authority: ctx.accounts.escrow_vault.to_account_info(),
            };
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.key(),
                    cpi_accounts,
                    &[seeds],
                ),
                leaf_amount,
            )?;
        }

        Ok(())
    }

    // ── 10. FINALIZE SETTLEMENT ──

    #[derive(Accounts)]
    pub struct FinalizeSettlement<'info> {
        #[account(
            mut,
            constraint = channel.status == ChannelStatus::Settling @ ChannelError::InvalidStatus,
        )]
        pub channel: Account<'info, ChannelAccount>,

        #[account(
            constraint = caller.key() == channel.user_pubkey
                || caller.key() == channel.provider_pubkey
                @ ChannelError::Unauthorized,
        )]
        pub caller: Signer<'info>,

        #[account(mut)]
        pub vault_a: Account<'info, TokenAccount>,

        #[account(mut)]
        pub vault_b: Account<'info, TokenAccount>,

        /// CHECK: PDA escrow vault
        #[account(
            mut,
            seeds = [b"escrow", channel.channel_id.as_ref()],
            bump,
        )]
        pub escrow_vault: UncheckedAccount<'info>,

        pub token_program: Program<'info, Token>,
        pub clock: Sysvar<'info, Clock>,

        /// CHECK: Instruction sysvar for ed25519 introspection
        pub instruction_sysvar: UncheckedAccount<'info>,
    }

    pub fn finalize_settlement(
        ctx: Context<FinalizeSettlement>,
    ) -> Result<()> {
        let channel = &mut ctx.accounts.channel;
        let current_slot = ctx.accounts.clock.slot;

        let deadline = channel.settle_deadline
            .ok_or(ChannelError::InvalidStatus)?;
        require!(current_slot >= deadline, ChannelError::SettlementNotExpired);

        let mut fin_msg = Vec::with_capacity(32 + 8 + 32);
        fin_msg.extend_from_slice(&channel.channel_id);
        fin_msg.extend_from_slice(&current_slot.to_le_bytes());
        fin_msg.extend_from_slice(&channel.current_root);

        let current_ix = read_current_ix_index(&ctx.accounts.instruction_sysvar)?;
        verify_ed25519_signature_introspection(
            &ctx.accounts.instruction_sysvar,
            &ctx.accounts.caller.key(),
            &fin_msg,
            current_ix,
        )?;

        let unclaimed = channel.total_deposited
            .checked_sub(channel.total_claimed)
            .ok_or(ChannelError::ArithmeticOverflow)?;

        if unclaimed > 0 {
            let total_deposit = channel.deposit_a
                .checked_add(channel.deposit_b)
                .ok_or(ChannelError::ArithmeticOverflow)?;

            if total_deposit > 0 {
                let ratio_a = (channel.deposit_a as u128)
                    .checked_mul(1_000_000)
                    .ok_or(ChannelError::ArithmeticOverflow)?
                    .checked_div(total_deposit as u128)
                    .ok_or(ChannelError::ArithmeticOverflow)?;

                let refund_a = (unclaimed as u128)
                    .checked_mul(ratio_a)
                    .ok_or(ChannelError::ArithmeticOverflow)?
                    .checked_div(1_000_000)
                    .ok_or(ChannelError::ArithmeticOverflow)? as u64;

                let refund_b = unclaimed.checked_sub(refund_a)
                    .ok_or(ChannelError::ArithmeticOverflow)?;

                let bump = ctx.bumps.escrow_vault;
                let seeds: &[&[u8]] = &[b"escrow", channel.channel_id.as_ref(), &[bump]];

                if refund_a > 0 {
                    let cpi_accounts_a = Transfer {
                        from: ctx.accounts.escrow_vault.to_account_info(),
                        to: ctx.accounts.vault_a.to_account_info(),
                        authority: ctx.accounts.escrow_vault.to_account_info(),
                    };
                    token::transfer(
                        CpiContext::new_with_signer(
                            ctx.accounts.token_program.key(),
                            cpi_accounts_a,
                            &[seeds],
                        ),
                        refund_a,
                    )?;
                }

                if refund_b > 0 {
                    let cpi_accounts_b = Transfer {
                        from: ctx.accounts.escrow_vault.to_account_info(),
                        to: ctx.accounts.vault_b.to_account_info(),
                        authority: ctx.accounts.escrow_vault.to_account_info(),
                    };
                    token::transfer(
                        CpiContext::new_with_signer(
                            ctx.accounts.token_program.key(),
                            cpi_accounts_b,
                            &[seeds],
                        ),
                        refund_b,
                    )?;
                }
            }
        }

        channel.status = ChannelStatus::Closed;

        Ok(())
    }
}

/// Verify ed25519 signature via instruction introspection.
/// Since signatures are no longer in instruction data, we don't have a specific signature
/// to match against. Instead, we verify that the runtime has validated an ed25519 signature
/// for the given pubkey and message by checking that the corresponding ed25519 instruction
/// exists and was executed.
/// Read the current instruction index from the instructions sysvar raw data.
/// The current index is stored as a u16 LE at the end of the sysvar data.
fn read_current_ix_index(instruction_sysvar: &UncheckedAccount<'_>) -> Result<u16> {
    let ix_account = instruction_sysvar.to_account_info();
    let data = ix_account.try_borrow_data()
        .map_err(|_| ChannelError::InvalidSignature)?;
    if data.len() < 2 {
        return Err(ChannelError::InvalidSignature.into());
    }
    let len = data.len();
    Ok(u16::from_le_bytes([data[len - 2], data[len - 1]]))
}

/// Verify ed25519 signature via instruction introspection.
///
/// Walks preceding ed25519 instructions in the transaction and checks if any contains
/// a matching pubkey and message. The runtime guarantees the signature was valid if
/// the ed25519 instruction executed successfully.
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
fn verify_ed25519_signature_introspection(
    instruction_sysvar: &UncheckedAccount<'_>,
    expected_pubkey: &Pubkey,
    message: &[u8],
    current_ix_index: u16,
) -> Result<()> {
    let ix_account = instruction_sysvar.to_account_info();
    let ix_data = ix_account.try_borrow_data()
        .map_err(|_| ChannelError::InvalidSignature)?;

    if ix_data.len() < 4 {
        return Err(ChannelError::InvalidSignature.into());
    }

    let num_instructions = u16::from_le_bytes([ix_data[0], ix_data[1]]) as usize;

    for ix_idx in 0..num_instructions {
        if ix_idx as u16 >= current_ix_index {
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

        // Parse instruction at ix_offset
        let mut pos = ix_offset;

        // Read num_accounts (u16)
        if pos + 2 > ix_data.len() - 2 {
            continue;
        }
        let num_accounts = u16::from_le_bytes([ix_data[pos], ix_data[pos + 1]]) as usize;
        pos += 2;

        // Skip accounts: each is u8 flags + 32 bytes pubkey = 33 bytes
        pos += num_accounts * 33;

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

        if pk_offset + 32 > data_len || msg_offset + msg_size > data_len {
            continue;
        }

        let pk = &ix_slice[pk_offset..pk_offset + 32];
        let msg = &ix_slice[msg_offset..msg_offset + msg_size];

        if pk == expected_pubkey.as_ref() && msg == message {
            return Ok(());
        }
    }

    Err(ChannelError::InvalidSignature.into())
}
