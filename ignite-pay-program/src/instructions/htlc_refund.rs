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
pub struct HtlcRefund<'info> {
    #[account(
        mut,
        constraint = channel.status == ChannelStatus::Challenged
            || channel.status == ChannelStatus::Settling
            @ ChannelError::InvalidStatus,
    )]
    pub channel: Account<'info, ChannelAccount>,

    /// CHECK: Must be the HTLC owner
    #[account(
        constraint = claimer.key() == channel.user_pubkey
            || claimer.key() == channel.provider_pubkey
            @ ChannelError::Unauthorized,
    )]
    pub claimer: Signer<'info>,

    /// Claimer's vault (SPL token account) to receive refunded funds.
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

/// HTLCRefund: owner claims refund after HTLC expires.
///
/// Validates: channel is Settling, current_slot > timelock_slot,
/// claimer is the leaf owner, Merkle proof, leaf_data integrity (BUG-40).
/// BUG-35 fix: includes SPL Token CPI transfer from escrow to claimer.
pub fn htlc_refund(
    ctx: Context<HtlcRefund>,
    leaf_index: u32,
    timelock_slot: u64,
    leaf_amount: u64,
    leaf_owner: Pubkey,
    leaf_hash: [u8; 32],
    proof: Vec<[u8; 32]>,
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

    // Verify claimer is the leaf owner
    require!(
        ctx.accounts.claimer.key() == leaf_owner,
        ChannelError::InvalidOwner
    );

    // Duplicate claim prevention
    require!(
        !channel.claimed_leaves.contains(&leaf_index),
        ChannelError::AlreadyClaimed
    );

    // BUG-40 fix: verify leaf_data integrity
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

        // Verify owner from leaf_data (offset 1, 32 bytes)
        let owner_bytes: [u8; 32] = leaf_data[1..33]
            .try_into()
            .map_err(|_| ChannelError::InvalidLeafData)?;
        let extracted_owner = Pubkey::new_from_array(owner_bytes);
        require!(
            extracted_owner == leaf_owner,
            ChannelError::InvalidLeafData
        );
    }

    // Verify HTLC has expired (strict >)
    require!(
        current_slot > timelock_slot,
        ChannelError::HtlcNotExpired
    );

    // Verify Merkle proof
    require!(
        verify_merkle_proof(&leaf_hash, &proof, &channel.current_root),
        ChannelError::ProofVerificationFailed
    );

    // PROG-8 fix: verify claimer's signature
    let mut refund_msg = Vec::with_capacity(32 + 8 + 32);
    refund_msg.extend_from_slice(&channel.channel_id);
    refund_msg.extend_from_slice(&current_slot.to_le_bytes());
    refund_msg.extend_from_slice(&channel.current_root);
    require!(
        verify_ed25519_signature(&refund_msg, &claimer_signature, &ctx.accounts.claimer.key()),
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

    // BUG-35 fix: SPL Token CPI transfer from escrow to claimer's vault
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
