use anchor_lang::prelude::*;

pub const GLOBAL_STATE_SEED: &[u8] = b"global_state";
pub const GLOBAL_VAULT_SEED: &[u8] = b"global_buyer_vault";
pub const CHANNEL_SEED: &[u8] = b"channel";
pub const SETTLEMENT_SEED: &[u8] = b"settlement";

#[account]
pub struct GlobalState {
    pub buyer: Pubkey,            // 32
    pub total_deposited: u64,     // 8
    pub total_allocated: u64,     // 8
    pub bump: u8,                 // 1
}

impl GlobalState {
    pub const SPACE: usize = 8 + 32 + 8 + 8 + 1; // 57
}

#[account]
pub struct Channel {
    pub buyer: Pubkey,            // 32
    pub merchant: Pubkey,         // 32
    pub spending_cap: u64,        // 8
    pub settled_amount: u64,      // 8
    pub nonce: u64,               // 8
    pub challenge_period: i64,    // 8
    pub dispute_period: i64,      // 8
    pub bump: u8,                 // 1
}

impl Channel {
    pub const SPACE: usize = 8 + 32 + 32 + 8 + 8 + 8 + 8 + 8 + 1; // 113
}

#[account]
pub struct SettlementEscrow {
    pub channel: Pubkey,          // 32
    pub merchant: Pubkey,         // 32
    pub amount: u64,              // 8
    pub merkle_root: [u8; 32],    // 32
    pub nonce: u64,               // 8
    pub created_at: i64,          // 8
    pub claimed: bool,            // 1
    pub disputed: bool,           // 1
    pub optimistic: bool,         // 1 — true if merchant-only submission
    pub bump: u8,                 // 1
}

impl SettlementEscrow {
    pub const SPACE: usize = 8 + 32 + 32 + 8 + 32 + 8 + 8 + 1 + 1 + 1 + 1; // 132
}
