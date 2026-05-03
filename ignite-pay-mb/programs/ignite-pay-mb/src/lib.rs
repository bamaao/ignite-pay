#![allow(unexpected_cfgs)]
pub mod error;
pub mod state;
pub mod utils;

use anchor_lang::prelude::*;
use anchor_lang::solana_program::system_instruction;
use solana_sha256_hasher::hashv;

use state::*;
use error::ErrorCode;
use utils::ed25519::{verify_ed25519_for_pubkey, VerifiedParty};
use utils::merkle::verify_sum_merkle_proof;

declare_id!("6pFXAg1oiV61wVvaJvMHqYdGMe2fscDwmN9UBUSvNuU3");

#[program]
pub mod ignite_pay_mb {
    use super::*;

    pub fn initialize_global(ctx: Context<InitializeGlobal>) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        global_state.buyer = ctx.accounts.buyer.key();
        global_state.total_deposited = 0;
        global_state.total_allocated = 0;
        global_state.bump = ctx.bumps.global_state;
        Ok(())
    }

    pub fn initialize_channel(
        ctx: Context<InitializeChannel>,
        spending_cap: u64,
        challenge_period: i64,
        dispute_period: i64,
    ) -> Result<()> {
        // Validate allocation: total_allocated + spending_cap <= total_deposited
        let global_state = &mut ctx.accounts.global_state;
        require!(
            global_state
                .total_allocated
                .checked_add(spending_cap)
                .ok_or(ErrorCode::ArithmeticOverflow)?
                <= global_state.total_deposited,
            ErrorCode::AllocationExceedsDeposit
        );
        global_state.total_allocated = global_state
            .total_allocated
            .checked_add(spending_cap)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        let buyer = ctx.accounts.buyer.key();
        let merchant = ctx.accounts.merchant.key();

        let channel = &mut ctx.accounts.channel;
        channel.buyer = buyer;
        channel.merchant = merchant;
        channel.spending_cap = spending_cap;
        channel.settled_amount = 0;
        channel.nonce = 0;
        channel.challenge_period = challenge_period;
        channel.dispute_period = dispute_period;
        channel.bump = ctx.bumps.channel;
        Ok(())
    }

    pub fn update_spending_cap(
        ctx: Context<UpdateSpendingCap>,
        new_spending_cap: u64,
    ) -> Result<()> {
        let channel = &mut ctx.accounts.channel;
        let global_state = &mut ctx.accounts.global_state;

        let delta = if new_spending_cap > channel.spending_cap {
            new_spending_cap - channel.spending_cap
        } else {
            0
        };

        require!(
            global_state
                .total_allocated
                .checked_add(delta)
                .ok_or(ErrorCode::ArithmeticOverflow)?
                <= global_state.total_deposited,
            ErrorCode::AllocationExceedsDeposit
        );

        global_state.total_allocated = global_state
            .total_allocated
            .checked_add(delta)
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        channel.spending_cap = new_spending_cap;
        Ok(())
    }

    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        let transfer_ix = system_instruction::transfer(
            &ctx.accounts.buyer.key(),
            &ctx.accounts.vault.key(),
            amount,
        );
        anchor_lang::solana_program::program::invoke(
            &transfer_ix,
            &[
                ctx.accounts.buyer.to_account_info(),
                ctx.accounts.vault.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;

        let global_state = &mut ctx.accounts.global_state;
        global_state.total_deposited = global_state
            .total_deposited
            .checked_add(amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        Ok(())
    }

    pub fn settle_batch(
        ctx: Context<BatchSettle>,
        merkle_root: [u8; 32],
        total_amount: u64,
        buyer_batch_sig: [u8; 64],
        merchant_batch_sig: [u8; 64],
    ) -> Result<()> {
        let channel = &ctx.accounts.channel;

        // Defense 1: spending cap
        let cumulative = channel
            .settled_amount
            .checked_add(total_amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        require!(
            cumulative <= channel.spending_cap,
            ErrorCode::SpendingCapExceeded
        );

        // Defense 2: global vault balance check
        let vault_lamports = ctx.accounts.vault.to_account_info().lamports();
        require!(
            total_amount <= vault_lamports,
            ErrorCode::InsufficientBalance
        );

        // Defense 3: dual signature verification via instruction introspection
        let mut msg_preimage = Vec::with_capacity(32 + 8 + 32 + 8);
        msg_preimage.extend_from_slice(&merkle_root);
        msg_preimage.extend_from_slice(&total_amount.to_le_bytes());
        msg_preimage.extend_from_slice(ctx.accounts.channel.key().as_ref());
        msg_preimage.extend_from_slice(&channel.nonce.to_le_bytes());
        let message_hash = hashv(&[&msg_preimage]);

        let current_ix = solana_instructions_sysvar::load_current_index_checked(
            &ctx.accounts.instruction_sysvar
        ).map_err(|_| ErrorCode::InvalidSignatureInstruction)? as usize;

        require!(
            current_ix >= 1,
            ErrorCode::InvalidTransactionLayout
        );

        let mut buyer_verified = false;
        let mut merchant_verified = false;
        for ix_idx in 0..current_ix {
            let result = verify_ed25519_for_pubkey(
                &ctx.accounts.instruction_sysvar,
                ix_idx,
                &message_hash.to_bytes(),
                &buyer_batch_sig,
                &merchant_batch_sig,
                &channel.buyer.to_bytes(),
                &channel.merchant.to_bytes(),
            );
            match result {
                Ok(VerifiedParty::Buyer) => buyer_verified = true,
                Ok(VerifiedParty::Merchant) => merchant_verified = true,
                _ => {}
            }
        }
        require!(buyer_verified, ErrorCode::BuyerSignatureNotFound);
        require!(merchant_verified, ErrorCode::MerchantSignatureNotFound);

        // Save values needed for invoke_signed before mutable borrow
        let channel_key = ctx.accounts.channel.key();
        let vault_key = ctx.accounts.vault.key();
        let escrow_key = ctx.accounts.settlement_escrow.key();
        let vault_bump = ctx.bumps.vault;
        let nonce = channel.nonce;
        let channel_merchant = ctx.accounts.channel.merchant;

        // Update channel state
        let channel = &mut ctx.accounts.channel;
        channel.settled_amount = channel
            .settled_amount
            .checked_add(total_amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        // Transfer from global vault -> settlement escrow
        let vault_seeds = &[
            GLOBAL_VAULT_SEED,
            channel.buyer.as_ref(),
            &[vault_bump],
        ];
        let transfer_ix = system_instruction::transfer(
            &vault_key,
            &escrow_key,
            total_amount,
        );
        anchor_lang::solana_program::program::invoke_signed(
            &transfer_ix,
            &[
                ctx.accounts.vault.to_account_info(),
                ctx.accounts.settlement_escrow.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[vault_seeds],
        )?;

        // Initialize settlement escrow
        let settlement = &mut ctx.accounts.settlement_escrow;
        settlement.channel = channel_key;
        settlement.merchant = channel_merchant;
        settlement.amount = total_amount;
        settlement.merkle_root = merkle_root;
        settlement.nonce = nonce;
        settlement.created_at = Clock::get()?.unix_timestamp;
        settlement.claimed = false;
        settlement.disputed = false;
        settlement.optimistic = false;
        settlement.bump = ctx.bumps.settlement_escrow;

        // Increment batch nonce
        channel.nonce = nonce
            .checked_add(1)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        Ok(())
    }

    /// Merchant-only optimistic settlement.
    /// Requires only the merchant's Ed25519 signature. Funds go to escrow
    /// and must wait for the full challenge_period before release.
    /// Challenge_period must be > 0 (no instant optimistic settlements).
    pub fn optimistic_settle(
        ctx: Context<OptimisticBatchSettle>,
        merkle_root: [u8; 32],
        total_amount: u64,
        merchant_batch_sig: [u8; 64],
    ) -> Result<()> {
        let channel = &ctx.accounts.channel;

        // Defense 1: spending cap
        let cumulative = channel
            .settled_amount
            .checked_add(total_amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        require!(
            cumulative <= channel.spending_cap,
            ErrorCode::SpendingCapExceeded
        );

        // Defense 2: global vault balance check
        let vault_lamports = ctx.accounts.vault.to_account_info().lamports();
        require!(
            total_amount <= vault_lamports,
            ErrorCode::InsufficientBalance
        );

        // Defense 3: challenge_period must be > 0 for optimistic settlements
        require!(
            channel.challenge_period > 0,
            ErrorCode::ChallengePeriodRequired
        );

        // Defense 4: merchant signature verification via instruction introspection
        let mut msg_preimage = Vec::with_capacity(32 + 8 + 32 + 8);
        msg_preimage.extend_from_slice(&merkle_root);
        msg_preimage.extend_from_slice(&total_amount.to_le_bytes());
        msg_preimage.extend_from_slice(ctx.accounts.channel.key().as_ref());
        msg_preimage.extend_from_slice(&channel.nonce.to_le_bytes());
        let message_hash = hashv(&[&msg_preimage]);

        let current_ix = solana_instructions_sysvar::load_current_index_checked(
            &ctx.accounts.instruction_sysvar
        ).map_err(|_| ErrorCode::InvalidSignatureInstruction)? as usize;

        require!(
            current_ix >= 1,
            ErrorCode::InvalidTransactionLayout
        );

        let mut merchant_verified = false;
        for ix_idx in 0..current_ix {
            let result = verify_ed25519_for_pubkey(
                &ctx.accounts.instruction_sysvar,
                ix_idx,
                &message_hash.to_bytes(),
                &[0u8; 64], // dummy buyer sig — not checked
                &merchant_batch_sig,
                &[0u8; 32], // dummy buyer pubkey — won't match
                &channel.merchant.to_bytes(),
            );
            if let Ok(VerifiedParty::Merchant) = result {
                merchant_verified = true;
            }
        }
        require!(merchant_verified, ErrorCode::MerchantSignatureNotFound);

        // Save values needed for invoke_signed before mutable borrow
        let channel_key = ctx.accounts.channel.key();
        let vault_key = ctx.accounts.vault.key();
        let escrow_key = ctx.accounts.settlement_escrow.key();
        let vault_bump = ctx.bumps.vault;
        let nonce = channel.nonce;
        let channel_merchant = ctx.accounts.channel.merchant;

        // Update channel state
        let channel = &mut ctx.accounts.channel;
        channel.settled_amount = channel
            .settled_amount
            .checked_add(total_amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        // Transfer from global vault -> settlement escrow
        let vault_seeds = &[
            GLOBAL_VAULT_SEED,
            channel.buyer.as_ref(),
            &[vault_bump],
        ];
        let transfer_ix = system_instruction::transfer(
            &vault_key,
            &escrow_key,
            total_amount,
        );
        anchor_lang::solana_program::program::invoke_signed(
            &transfer_ix,
            &[
                ctx.accounts.vault.to_account_info(),
                ctx.accounts.settlement_escrow.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[vault_seeds],
        )?;

        // Initialize settlement escrow
        let settlement = &mut ctx.accounts.settlement_escrow;
        settlement.channel = channel_key;
        settlement.merchant = channel_merchant;
        settlement.amount = total_amount;
        settlement.merkle_root = merkle_root;
        settlement.nonce = nonce;
        settlement.created_at = Clock::get()?.unix_timestamp;
        settlement.claimed = false;
        settlement.disputed = false;
        settlement.optimistic = true;
        settlement.bump = ctx.bumps.settlement_escrow;

        // Increment batch nonce
        channel.nonce = nonce
            .checked_add(1)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        Ok(())
    }

    pub fn release_settlement(ctx: Context<ReleaseSettlement>) -> Result<()> {
        // Read-only checks first
        let settlement = &ctx.accounts.settlement_escrow;
        require!(!settlement.claimed, ErrorCode::AlreadyClaimed);
        require!(!settlement.disputed, ErrorCode::Disputed);

        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= settlement.created_at + ctx.accounts.channel.challenge_period,
            ErrorCode::ChallengePeriodNotExpired
        );

        // Mark as claimed
        ctx.accounts.settlement_escrow.claimed = true;

        // Transfer settlement amount from escrow PDA to merchant
        let escrow_info = ctx.accounts.settlement_escrow.to_account_info();
        let merchant_info = ctx.accounts.merchant.to_account_info();
        let amount = ctx.accounts.settlement_escrow.amount;
        **escrow_info.lamports.borrow_mut() = escrow_info
            .lamports()
            .checked_sub(amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        **merchant_info.lamports.borrow_mut() = merchant_info
            .lamports()
            .checked_add(amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        Ok(())
    }

    pub fn dispute(ctx: Context<Dispute>) -> Result<()> {
        let settlement = &mut ctx.accounts.settlement_escrow;
        require!(!settlement.claimed, ErrorCode::AlreadyClaimed);
        require!(!settlement.disputed, ErrorCode::AlreadyDisputed);

        let now = Clock::get()?.unix_timestamp;
        require!(
            now < settlement.created_at + ctx.accounts.channel.challenge_period,
            ErrorCode::ChallengePeriodExpired
        );

        require!(
            ctx.accounts.buyer.key() == ctx.accounts.channel.buyer,
            ErrorCode::NotBuyer
        );

        settlement.disputed = true;
        Ok(())
    }

    pub fn force_release(ctx: Context<ForceRelease>) -> Result<()> {
        let settlement = &ctx.accounts.settlement_escrow;
        require!(settlement.disputed, ErrorCode::NotDisputed);
        require!(!settlement.claimed, ErrorCode::AlreadyClaimed);

        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= settlement.created_at + ctx.accounts.channel.dispute_period,
            ErrorCode::DisputePeriodNotExpired
        );

        ctx.accounts.settlement_escrow.claimed = true;

        // Transfer settlement amount from escrow PDA to merchant
        let escrow_info = ctx.accounts.settlement_escrow.to_account_info();
        let merchant_info = ctx.accounts.merchant.to_account_info();
        let amount = ctx.accounts.settlement_escrow.amount;
        **escrow_info.lamports.borrow_mut() = escrow_info
            .lamports()
            .checked_sub(amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        **merchant_info.lamports.borrow_mut() = merchant_info
            .lamports()
            .checked_add(amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        Ok(())
    }

    pub fn resolve_dispute(
        ctx: Context<ResolveDispute>,
        voucher_seq: u64,
        voucher_amount: u64,
        buyer_voucher_sig: [u8; 64],
        sibling_hashes: Vec<[u8; 32]>,
        sibling_sums: Vec<u64>,
    ) -> Result<()> {
        // Read-only checks first
        let settlement = &ctx.accounts.settlement_escrow;
        require!(settlement.disputed, ErrorCode::NotDisputed);
        require!(!settlement.claimed, ErrorCode::AlreadyClaimed);
        require!(
            ctx.accounts.buyer.key() == ctx.accounts.channel.buyer,
            ErrorCode::NotBuyer
        );
        require!(
            sibling_hashes.len() == sibling_sums.len(),
            ErrorCode::InvalidFraudProof
        );

        // Compute leaf hash (domain separator 0x00)
        let mut leaf_preimage = Vec::with_capacity(1 + 32 + 8 + 8 + 32 + 64);
        leaf_preimage.push(0x00);
        leaf_preimage.extend_from_slice(ctx.accounts.channel.key().as_ref());
        leaf_preimage.extend_from_slice(&voucher_seq.to_le_bytes());
        leaf_preimage.extend_from_slice(&voucher_amount.to_le_bytes());
        leaf_preimage.extend_from_slice(ctx.accounts.channel.buyer.as_ref());
        leaf_preimage.extend_from_slice(&buyer_voucher_sig);
        let leaf_hash = hashv(&[&leaf_preimage]).to_bytes();

        // Verify sum merkle proof
        let (root_matches, _computed_total) = verify_sum_merkle_proof(
            &leaf_hash,
            voucher_amount,
            &sibling_hashes,
            &sibling_sums,
            &settlement.merkle_root,
        )?;

        require!(root_matches, ErrorCode::InvalidFraudProof);
        require!(
            voucher_amount < settlement.amount,
            ErrorCode::FraudNotProven
        );

        let amount = settlement.amount;
        let settled_amount = ctx.accounts.channel.settled_amount;

        // Rollback settled_amount
        let channel = &mut ctx.accounts.channel;
        channel.settled_amount = settled_amount
            .checked_sub(amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        ctx.accounts.settlement_escrow.claimed = true;

        // Transfer settlement amount from escrow PDA to buyer
        let escrow_info = ctx.accounts.settlement_escrow.to_account_info();
        let buyer_info = ctx.accounts.buyer.to_account_info();
        let escrow_amount = ctx.accounts.settlement_escrow.amount;
        **escrow_info.lamports.borrow_mut() = escrow_info
            .lamports()
            .checked_sub(escrow_amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        **buyer_info.lamports.borrow_mut() = buyer_info
            .lamports()
            .checked_add(escrow_amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        let global_state = &ctx.accounts.global_state;

        // Can only withdraw unallocated funds
        let unallocated = global_state
            .total_deposited
            .checked_sub(global_state.total_allocated)
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        require!(amount <= unallocated, ErrorCode::InsufficientBalance);

        // Save values for seeds
        let buyer_key = ctx.accounts.global_state.buyer;
        let vault_bump = ctx.bumps.vault;

        // Update global state
        let global_state = &mut ctx.accounts.global_state;
        global_state.total_deposited = global_state
            .total_deposited
            .checked_sub(amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        let vault_seeds = &[
            GLOBAL_VAULT_SEED,
            buyer_key.as_ref(),
            &[vault_bump],
        ];
        let transfer_ix = system_instruction::transfer(
            &ctx.accounts.vault.key(),
            &ctx.accounts.buyer.key(),
            amount,
        );
        anchor_lang::solana_program::program::invoke_signed(
            &transfer_ix,
            &[
                ctx.accounts.vault.to_account_info(),
                ctx.accounts.buyer.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[vault_seeds],
        )?;

        Ok(())
    }
}

// ── Account Structs ───────────────────────────────────────────

#[derive(Accounts)]
pub struct InitializeGlobal<'info> {
    #[account(
        init,
        payer = buyer,
        space = GlobalState::SPACE,
        seeds = [GLOBAL_STATE_SEED, buyer.key().as_ref()],
        bump,
    )]
    pub global_state: Account<'info, GlobalState>,
    /// CHECK: Global vault PDA — not init'd, stays System Program owned
    #[account(
        seeds = [GLOBAL_VAULT_SEED, buyer.key().as_ref()],
        bump,
    )]
    pub vault: UncheckedAccount<'info>,
    #[account(mut)]
    pub buyer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(spending_cap: u64, challenge_period: i64, dispute_period: i64)]
pub struct InitializeChannel<'info> {
    #[account(
        mut,
        has_one = buyer,
        seeds = [GLOBAL_STATE_SEED, buyer.key().as_ref()],
        bump = global_state.bump,
    )]
    pub global_state: Account<'info, GlobalState>,
    #[account(
        init,
        payer = buyer,
        space = Channel::SPACE,
        seeds = [CHANNEL_SEED, buyer.key().as_ref(), merchant.key().as_ref()],
        bump,
    )]
    pub channel: Account<'info, Channel>,
    #[account(mut)]
    pub buyer: Signer<'info>,
    /// CHECK: Trusted merchant account
    pub merchant: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(new_spending_cap: u64)]
pub struct UpdateSpendingCap<'info> {
    #[account(
        mut,
        has_one = buyer,
        seeds = [GLOBAL_STATE_SEED, buyer.key().as_ref()],
        bump = global_state.bump,
    )]
    pub global_state: Account<'info, GlobalState>,
    #[account(
        mut,
        has_one = buyer,
        seeds = [CHANNEL_SEED, channel.buyer.as_ref(), channel.merchant.as_ref()],
        bump = channel.bump,
    )]
    pub channel: Account<'info, Channel>,
    #[account(mut)]
    pub buyer: Signer<'info>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(
        mut,
        has_one = buyer,
        seeds = [GLOBAL_STATE_SEED, buyer.key().as_ref()],
        bump = global_state.bump,
    )]
    pub global_state: Account<'info, GlobalState>,
    /// CHECK: Global vault PDA — seeds bind it to this buyer, owner is System Program
    #[account(
        mut,
        seeds = [GLOBAL_VAULT_SEED, buyer.key().as_ref()],
        bump,
    )]
    pub vault: UncheckedAccount<'info>,
    #[account(mut)]
    pub buyer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct BatchSettle<'info> {
    #[account(
        mut,
        seeds = [GLOBAL_STATE_SEED, channel.buyer.as_ref()],
        bump = global_state.bump,
    )]
    pub global_state: Account<'info, GlobalState>,
    /// CHECK: Global vault PDA bound to buyer, owner is System Program
    #[account(
        mut,
        seeds = [GLOBAL_VAULT_SEED, channel.buyer.as_ref()],
        bump,
    )]
    pub vault: UncheckedAccount<'info>,
    #[account(
        mut,
        has_one = merchant,
        seeds = [CHANNEL_SEED, channel.buyer.as_ref(), merchant.key().as_ref()],
        bump = channel.bump,
    )]
    pub channel: Account<'info, Channel>,
    /// CHECK: Settlement escrow — one per batch
    #[account(
        init,
        payer = merchant,
        space = SettlementEscrow::SPACE,
        seeds = [SETTLEMENT_SEED, channel.key().as_ref(), channel.nonce.to_le_bytes().as_ref()],
        bump,
    )]
    pub settlement_escrow: Account<'info, SettlementEscrow>,
    #[account(mut)]
    pub merchant: Signer<'info>,
    /// CHECK: Instructions sysvar for Ed25519 signature introspection
    #[account(address = solana_instructions_sysvar::ID)]
    pub instruction_sysvar: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct OptimisticBatchSettle<'info> {
    #[account(
        mut,
        seeds = [GLOBAL_STATE_SEED, channel.buyer.as_ref()],
        bump = global_state.bump,
    )]
    pub global_state: Account<'info, GlobalState>,
    /// CHECK: Global vault PDA bound to buyer, owner is System Program
    #[account(
        mut,
        seeds = [GLOBAL_VAULT_SEED, channel.buyer.as_ref()],
        bump,
    )]
    pub vault: UncheckedAccount<'info>,
    #[account(
        mut,
        has_one = merchant,
        seeds = [CHANNEL_SEED, channel.buyer.as_ref(), merchant.key().as_ref()],
        bump = channel.bump,
    )]
    pub channel: Account<'info, Channel>,
    /// CHECK: Settlement escrow — one per batch
    #[account(
        init,
        payer = merchant,
        space = SettlementEscrow::SPACE,
        seeds = [SETTLEMENT_SEED, channel.key().as_ref(), channel.nonce.to_le_bytes().as_ref()],
        bump,
    )]
    pub settlement_escrow: Account<'info, SettlementEscrow>,
    #[account(mut)]
    pub merchant: Signer<'info>,
    /// CHECK: Instructions sysvar for Ed25519 signature introspection
    #[account(address = solana_instructions_sysvar::ID)]
    pub instruction_sysvar: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ReleaseSettlement<'info> {
    #[account(mut, has_one = merchant)]
    pub channel: Account<'info, Channel>,
    #[account(
        mut,
        seeds = [SETTLEMENT_SEED, channel.key().as_ref(), settlement_escrow.nonce.to_le_bytes().as_ref()],
        bump = settlement_escrow.bump,
        constraint = !settlement_escrow.claimed @ ErrorCode::AlreadyClaimed,
        constraint = !settlement_escrow.disputed @ ErrorCode::Disputed,
    )]
    pub settlement_escrow: Account<'info, SettlementEscrow>,
    #[account(mut)]
    pub merchant: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Dispute<'info> {
    #[account(mut)]
    pub channel: Account<'info, Channel>,
    #[account(
        mut,
        seeds = [SETTLEMENT_SEED, channel.key().as_ref(), settlement_escrow.nonce.to_le_bytes().as_ref()],
        bump = settlement_escrow.bump,
        constraint = !settlement_escrow.claimed @ ErrorCode::AlreadyClaimed,
        constraint = !settlement_escrow.disputed @ ErrorCode::AlreadyDisputed,
    )]
    pub settlement_escrow: Account<'info, SettlementEscrow>,
    #[account(mut)]
    pub buyer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ForceRelease<'info> {
    #[account(mut, has_one = merchant)]
    pub channel: Account<'info, Channel>,
    #[account(
        mut,
        seeds = [SETTLEMENT_SEED, channel.key().as_ref(), settlement_escrow.nonce.to_le_bytes().as_ref()],
        bump = settlement_escrow.bump,
        constraint = settlement_escrow.disputed @ ErrorCode::NotDisputed,
        constraint = !settlement_escrow.claimed @ ErrorCode::AlreadyClaimed,
    )]
    pub settlement_escrow: Account<'info, SettlementEscrow>,
    #[account(mut)]
    pub merchant: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ResolveDispute<'info> {
    #[account(mut)]
    pub channel: Account<'info, Channel>,
    #[account(
        mut,
        seeds = [SETTLEMENT_SEED, channel.key().as_ref(), settlement_escrow.nonce.to_le_bytes().as_ref()],
        bump = settlement_escrow.bump,
        constraint = settlement_escrow.disputed @ ErrorCode::NotDisputed,
        constraint = !settlement_escrow.claimed @ ErrorCode::AlreadyClaimed,
    )]
    pub settlement_escrow: Account<'info, SettlementEscrow>,
    #[account(mut)]
    pub buyer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(
        mut,
        has_one = buyer,
        seeds = [GLOBAL_STATE_SEED, buyer.key().as_ref()],
        bump = global_state.bump,
    )]
    pub global_state: Account<'info, GlobalState>,
    /// CHECK: Global vault PDA bound to buyer, owner is System Program
    #[account(
        mut,
        seeds = [GLOBAL_VAULT_SEED, buyer.key().as_ref()],
        bump,
    )]
    pub vault: UncheckedAccount<'info>,
    #[account(mut)]
    pub buyer: Signer<'info>,
    pub system_program: Program<'info, System>,
}
