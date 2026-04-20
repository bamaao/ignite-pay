use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use crate::state::{ChannelAccount, ChannelStatus};
use crate::error::ChannelError;
use crate::utils::merkle::verify_merkle_proof;
use crate::utils::ed25519::verify_ed25519_signature;

/// Leaf type discriminator for HTLC leaves.
/// UTXOLeaf borsh layout: u8(leaf_type) + 32 bytes(owner Pubkey) + u64(amount) + ...
const LEAF_TYPE_HTLC: u8 = 1;

#[derive(Accounts)]
pub struct VerifyHtlc<'info> {
    #[account(
        mut,
        constraint = channel.status == ChannelStatus::Challenged
            || channel.status == ChannelStatus::Settling
            @ ChannelError::InvalidStatus,
    )]
    pub channel: Account<'info, ChannelAccount>,

    /// CHECK: Must be the HTLC beneficiary
    #[account(
        constraint = claimer.key() == channel.user_pubkey
            || claimer.key() == channel.provider_pubkey
            @ ChannelError::Unauthorized,
    )]
    pub claimer: Signer<'info>,

    /// Beneficiary's vault (SPL token account) to receive claimed funds.
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

/// VerifyHTLC: beneficiary claims an HTLC leaf by providing the preimage.
///
/// Validates: channel is Challenged/Settling, settle_deadline, preimage matches hash_lock,
/// timelock not expired (current_slot <= timelock_slot), claimer is beneficiary,
/// Merkle proof, leaf_data integrity (BUG-39, BUG-40).
/// BUG-35 fix: includes SPL Token CPI transfer from escrow to beneficiary.
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
    claimer_signature: [u8; 64],
) -> Result<()> {
    let channel = &mut ctx.accounts.channel;
    let current_slot = ctx.accounts.clock.slot;

    // Check deadline: in Challenged state, derive from challenge_slot + challenge_duration;
    // in Settling state, use settle_deadline.
    let deadline = if channel.status == ChannelStatus::Challenged {
        let cs = channel.challenge_slot
            .ok_or(ChannelError::InvalidStatus)?;
        cs.checked_add(channel.challenge_duration)
            .ok_or(ChannelError::ArithmeticOverflow)?
    } else {
        channel.settle_deadline
            .ok_or(ChannelError::InvalidStatus)?
    };
    require!(
        current_slot <= deadline,
        ChannelError::SettlementExpired
    );

    // Verify claimer is the beneficiary
    require!(
        ctx.accounts.claimer.key() == beneficiary,
        ChannelError::InvalidOwner
    );

    // Duplicate claim prevention
    require!(
        !channel.claimed_leaves.contains(&leaf_index),
        ChannelError::AlreadyClaimed
    );

    // BUG-39 / BUG-40 fix: verify leaf_data integrity before Merkle proof
    // Deserialize leaf_data and verify hash_lock, timelock_slot, beneficiary, leaf_type
    {
        let computed_hash = anchor_lang::solana_program::hash::hash(&leaf_data).to_bytes();
        require!(
            computed_hash == leaf_hash,
            ChannelError::InvalidLeafData
        );

        // UTXOLeaf borsh layout: u8(leaf_type) + 32 bytes(owner Pubkey) + u64(amount) + ...
        require!(
            leaf_data.len() >= 41,
            ChannelError::InvalidLeafData
        );

        // Verify leaf_type is HTLC
        let leaf_type = leaf_data[0];
        require!(
            leaf_type == LEAF_TYPE_HTLC,
            ChannelError::InvalidLeafData
        );

        // Extract amount from leaf_data (offset 33, 8 bytes LE) and verify
        let amount_bytes: [u8; 8] = leaf_data[33..41]
            .try_into()
            .map_err(|_| ChannelError::InvalidLeafData)?;
        let extracted_amount = u64::from_le_bytes(amount_bytes);
        require!(
            extracted_amount == leaf_amount,
            ChannelError::AmountMismatch
        );

        // Verify beneficiary from leaf_data (offset 1, 32 bytes)
        let beneficiary_bytes: [u8; 32] = leaf_data[1..33]
            .try_into()
            .map_err(|_| ChannelError::InvalidLeafData)?;
        let extracted_beneficiary = Pubkey::new_from_array(beneficiary_bytes);
        require!(
            extracted_beneficiary == beneficiary,
            ChannelError::InvalidLeafData
        );
    }

    // Verify preimage matches hash_lock
    let computed_hash = anchor_lang::solana_program::hash::hash(&preimage).to_bytes();
    require!(
        computed_hash == hash_lock,
        ChannelError::HashLockMismatch
    );

    // BUG-18 fix: verify HTLC has not expired (current_slot must be <= timelock_slot)
    require!(
        current_slot <= timelock_slot,
        ChannelError::HtlcExpired
    );

    // Verify Merkle proof
    require!(
        verify_merkle_proof(&leaf_hash, &proof, &channel.current_root),
        ChannelError::ProofVerificationFailed
    );

    // PROG-8 fix: verify claimer's signature
    let mut htlc_msg = Vec::with_capacity(32 + 8 + 32);
    htlc_msg.extend_from_slice(&channel.channel_id);
    htlc_msg.extend_from_slice(&current_slot.to_le_bytes());
    htlc_msg.extend_from_slice(&channel.current_root);
    require!(
        verify_ed25519_signature(&htlc_msg, &claimer_signature, &ctx.accounts.claimer.key()),
        ChannelError::InvalidSignature
    );

    // Update total_claimed with overflow protection
    channel.total_claimed = channel.total_claimed
        .checked_add(leaf_amount)
        .ok_or(ChannelError::ArithmeticOverflow)?;

    require!(
        channel.total_claimed <= channel.total_deposited,
        ChannelError::AmountConservation
    );

    // Mark leaf as claimed
    channel.claimed_leaves.push(leaf_index);

    // BUG-35 fix: SPL Token CPI transfer from escrow to beneficiary's vault
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
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts,
                &[seeds],
            ),
            leaf_amount,
        )?;
    }

    Ok(())
}
