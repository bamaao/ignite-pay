use anchor_lang::prelude::*;
use anchor_spl::token::{Token, TokenAccount};
use crate::state::{ChannelAccount, ChannelStatus};
use crate::error::ChannelError;
use crate::utils::ed25519::verify_ed25519_signature;

#[derive(Accounts)]
#[instruction(
    channel_id: [u8; 32],
    tree_depth: u32,
    open_slot: u64,
    challenge_duration: u64,
    min_challenge_delay: u64,
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

    /// User who is opening the channel. Must match the provided user_pubkey.
    #[account(
        constraint = user.key() == user_pubkey.key() @ ChannelError::Unauthorized,
    )]
    pub user: Signer<'info>,

    /// CHECK: User's pubkey - the user Signer above must match this
    pub user_pubkey: AccountInfo<'info>,

    /// CHECK: Provider's pubkey, verified off-chain
    pub provider_pubkey: AccountInfo<'info>,

    /// The SPL token mint for this channel.
    pub token_mint: AccountInfo<'info>,

    /// User's vault (SPL token account).
    #[account(
        constraint = vault_a.mint == token_mint.key() @ ChannelError::InvalidOwner,
    )]
    pub vault_a: Account<'info, TokenAccount>,

    /// Provider's vault (SPL token account).
    #[account(
        constraint = vault_b.mint == token_mint.key() @ ChannelError::InvalidOwner,
    )]
    pub vault_b: Account<'info, TokenAccount>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
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
    sig_a: [u8; 64],
) -> Result<()> {
    // DEV-18 / PROG-14 fix: validate tree_depth BEFORE space() is used.
    // The space() is evaluated via the #[account] macro above, but tree_depth
    // is capped at 8 inside ChannelAccount::space(). We still enforce the
    // constraint here so that invalid tree_depth values are rejected explicitly.
    require!(
        tree_depth <= 8,
        ChannelError::LeafIndexOutOfBounds
    );

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
    channel.leaf_count = 1; // Initial root leaf
    channel.claimed_leaves = Vec::new();
    channel.auto_close_slot = None;

    // BUG-34 fix: verify user's signature on the open_channel parameters
    // Message = channel_id || deposit_a || tree_depth || initial_root
    let mut msg = Vec::with_capacity(32 + 8 + 4 + 32);
    msg.extend_from_slice(&channel_id);
    msg.extend_from_slice(&deposit_a.to_le_bytes());
    msg.extend_from_slice(&tree_depth.to_le_bytes());
    msg.extend_from_slice(&initial_root);

    require!(
        verify_ed25519_signature(&msg, &sig_a, &channel.user_pubkey),
        ChannelError::InvalidSignature
    );

    Ok(())
}
