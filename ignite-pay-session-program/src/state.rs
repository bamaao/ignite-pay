use anchor_lang::prelude::*;

/// On-chain account storing session key state.
///
/// PDA seeds: ["session", owner.as_ref(), ephemeral_signer.as_ref()]
#[account]
pub struct SessionKeyAccount {
    /// Owner (payer) public key — the user who registered this session.
    pub owner: Pubkey,
    /// Ephemeral signer public key — the session key that can execute payments.
    pub ephemeral_signer: Pubkey,
    /// Target program that this session is authorized to interact with.
    pub target_program: Pubkey,
    /// SPL Token mint for this session. Pubkey::default() means SOL session.
    pub token_mint: Pubkey,
    /// Unix timestamp when this session expires.
    pub expires_at: i64,
    /// Maximum cumulative spending limit in lamports.
    pub spending_limit: u64,
    /// Cumulative amount spent so far in lamports.
    pub current_spent: u64,
    /// Permission scopes (e.g., ["sol:transfer", "spl:transfer"]).
    pub scopes: Vec<String>,
    /// Whether this session has been revoked.
    pub revoked: bool,
    /// PDA bump seed.
    pub bump: u8,
}

impl SessionKeyAccount {
    /// Calculate the space required for a SessionKeyAccount.
    ///
    /// - Vec<String>: 4 (length prefix) + max_scopes * (4 + max_scope_len)
    /// - Using max 10 scopes of 32 chars each as reasonable upper bound.
    pub fn space(max_scopes: usize) -> usize {
        8 + // discriminator
        32 + // owner
        32 + // ephemeral_signer
        32 + // target_program
        32 + // token_mint
        8 + // expires_at
        8 + // spending_limit
        8 + // current_spent
        4 + (max_scopes * (4 + 32)) + // scopes: Vec<String> with max 10 entries of 32 chars
        1 + // revoked
        1 // bump
    }

    /// Default space with up to 10 scopes.
    pub fn default_space() -> usize {
        Self::space(10)
    }
}
