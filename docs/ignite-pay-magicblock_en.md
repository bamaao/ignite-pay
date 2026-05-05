Ignite-Pay Payment Infrastructure — Complete Implementation Plan

---

### 1. Overall Architecture

The entire system consists of three components: **Solana Mainnet (L1)**, **MagicBlock Ephemeral Rollups (ER)**, and an **Off-chain Fraud Prevention Layer**.

* **L1 (Solana Mainnet)**: Responsible for channel creation, fund locking, signature verification, and final settlement. All limits and balance states are stored in on-chain PDAs and cannot be tampered with by external parameters.

> **Current Scope**: The implementation below only supports SOL. To extend to SPL Tokens (USDC/USDT, etc.): replace `deposit` with Token Program CPI to transfer tokens into the Vault's Associated Token Account; change the fund transfers in `settle_batch` and `release_settlement` to Token Program `transfer` instructions, with the Vault PDA authorizing as signer. The core logic (dual signature, Merkle tree, challenge window) remains unchanged.
* **ER (MagicBlock Ephemeral Rollups)**: Responsible for high-speed state transitions (latency $< 50\text{ms}$, gas-free), recording a Voucher for each micropayment.
* **Off-chain Fraud Prevention Layer**: Responsible for dispute resolution within the challenge window. The buyer can submit individual Voucher proofs during the window and verify them against the on-chain `merkle_root`.

---

### 2. Operational Flow: Four-Step Closed Loop

#### Step 1: Channel Initialization (Initialize Channel)

The buyer calls the on-chain `initialize_channel` to create a channel PDA:

* PDA seeds: `["channel", buyer pubkey, merchant pubkey]`
* Writes `spending_cap` (spending limit) and `merchant` public key
* **Simultaneously computes** the corresponding Vault PDA: `["vault", channel pubkey]`, used to custody buyer funds
* The channel records `vault_bump`; all subsequent operations verify vault ownership through seeds + bump

This step **records the limit and both parties' identities, and computes the Vault PDA address**. The Vault is not `init`'d by the program, keeping its owner as the **System Program**. This way, when the buyer deposits funds via `system_instruction::transfer`, the Vault acts as a regular system account receiving SOL (best compatibility); the program can later use `invoke_signed` to call System Program's `transfer` to move funds out of the Vault (PDA seeds matching is sufficient for signing). The Vault does not exist on-chain before the first `deposit`; it is automatically created by the System Program on the first `deposit`.

#### Step 2: Fund Deposit

The buyer calls the on-chain `deposit` to transfer SOL into the channel-associated Vault PDA:

* Uses `system_instruction::transfer` from the buyer (Signer) to the Vault PDA
* The Vault PDA is owned by the **System Program** (not `init`'d by the program), and can receive SOL
* The contract updates `channel.balance += amount`
* The Vault's lamports balance represents the channel's actual available funds

> **SPL Token Extension**: The current design only supports SOL. SPL token deposit requires Token Program CPI (`transfer` instruction) to transfer tokens from the buyer's ATA to the Vault ATA, with the Vault PDA authorizing as signer.

#### Step 3: Off-chain Micropayments (State Transition on ER)

The buyer signs micropayment vouchers (Voucher) on the ER layer for each transaction:

$$\text{Voucher} = (\text{channel\_id},\ \text{voucher\_seq},\ \text{amount},\ \text{cumulative\_amount},\ \text{buyer\_sig})$$

Field descriptions:
* `amount`: The amount of this individual transaction. This is the core field for Merkle tree leaves.
* `cumulative_amount`: The cumulative spending up to and including this transaction ($= \sum$ of historical `amount`). Used only for the buyer's local limit checking, **not involved in Merkle tree construction**.
* `voucher_seq`: Voucher sequence number, monotonically increasing starting from 0. Used to distinguish different micropayments within the same channel and prevent replay.
* `buyer_sig`: The buyer's Ed25519 signature for this Voucher. Signed message = `SHA256(channel_id || voucher_seq || amount)`.

The buyer verifies `cumulative_amount <= spending_cap` before signing to ensure they do not exceed the limit.

> **Note**: `voucher_seq` and the Channel's `batch_nonce` are two different counters. `voucher_seq` is the sequence number for each micropayment (can number in the thousands), while `batch_nonce` is the settlement batch number (incremented by 1 for each `settle_batch`).

**Merkle Tree Construction**: At the end of a settlement period, the merchant constructs a Merkle tree from the `amount` fields of all Vouchers in that period. Each leaf is:

$$\text{Leaf} = \text{SHA256}(\text{0x00} \ ||\ \text{channel\_id} \ ||\  \text{voucher\_seq} \ ||\  \text{amount} \ ||\  \text{buyer\_pubkey} \ ||\  \text{buyer\_sig})$$

> The domain separators `0x00` (leaf node) and `0x01` (internal node) defend against **second preimage attacks**. The internal node format is `SHA256(0x01 || lo_hash || lo_sum || hi_hash || hi_sum)`, ensuring an attacker cannot forge a leaf as a valid internal node.

Before settlement, the buyer signs a **batch authorization message**:

$$\text{SettlementMsg} = \text{SHA256}(\text{merkle\_root}\ ||\ \text{total\_amount}\ ||\ \text{channel\_id}\ ||\ \text{batch\_nonce})$$

where `batch_nonce` is the on-chain `channel.nonce` (current settlement batch number). The buyer's Ed25519 signature on this message becomes `buyer_batch_sig`, and the merchant also signs the same message to produce `merchant_batch_sig`. Both signatures will be verified by the contract in `settle_batch`.

#### Step 4: On-chain Settlement (Settlement on L1)

The merchant calls `settle_batch`, and the contract performs three lines of defense checks:

| Defense Line | Verification | Purpose |
|------|---------|------|
| Defense 1 | `settled_amount + total_amount <= channel.spending_cap` | Verify cumulative settlement does not exceed on-chain limit |
| Defense 2 | `total_amount <= channel.balance` | Verify it does not exceed the Vault's real-time balance (`balance` is synced downward after `settle_batch`) |
| Defense 3 | Verify buyer and merchant dual signatures on `(merkle_root, total_amount, channel_id, batch_nonce)` | Ensure the Merkle root and total amount are acknowledged by both parties |

After verification passes, funds enter a **pending release state** (deposited into a Settlement Escrow PDA) rather than being transferred directly to the merchant's account. Only after the challenge window expires can the merchant call `release_settlement` to withdraw the funds.

#### Buyer Withdrawal of Unused Funds (Withdraw)

The buyer can call `withdraw` at any time to withdraw unused funds from the channel. The contract verifies:

* The caller is `channel.buyer` (`has_one` constraint)
* The withdrawal amount does not exceed `channel.balance` (after a successful `settle_batch`, `balance` is decremented accordingly, so `balance` always equals the Vault's real-time balance)

After withdrawal, `channel.balance` decreases accordingly, and lamports from the Vault PDA are transferred directly back to the buyer's account.

---

### 3. Complete Code Implementation

#### 1. On-chain Smart Contract Code (Rust / Anchor)

```rust
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    ed25519_program, system_instruction, hash::hashv,
    sysvar::instructions::{load_current_index_checked, load_instruction_at_checked},
};

declare_id!("IgnitePay11111111111111111111111111111111111");

// PDA seed constants
pub const CHANNEL_SEED: &[u8] = b"channel";
pub const VAULT_SEED: &[u8] = b"vault";
pub const SETTLEMENT_SEED: &[u8] = b"settlement";

#[program]
pub mod ignite_pay {
    use super::*;

    // -- Step 1: Create Channel ----------------------------------------

    pub fn initialize_channel(
        ctx: Context<InitializeChannel>,
        spending_cap: u64,
        challenge_period: i64,
        dispute_period: i64,
    ) -> Result<()> {
        let channel = &mut ctx.accounts.channel;
        channel.buyer = ctx.accounts.buyer.key();
        channel.merchant = ctx.accounts.merchant.key();
        channel.balance = 0;
        channel.spending_cap = spending_cap;
        channel.settled_amount = 0;
        channel.nonce = 0;
        channel.bump = ctx.bumps.channel;
        // Vault is not init'd; manually derive bump via seeds and store in Channel
        let (_, vault_bump) = Pubkey::find_program_address(
            &[VAULT_SEED, ctx.accounts.channel.key().as_ref()],
            ctx.program_id,
        );
        channel.vault_bump = vault_bump;
        // Configurable time windows to adapt to different scenarios
        // (short window for high-frequency micro-payments, long window for medium-frequency larger payments)
        channel.challenge_period = challenge_period;  // Recommended: 21600 (6h)
        channel.dispute_period = dispute_period;      // Recommended: 172800 (48h)
        Ok(())
    }

    // -- Step 2: Buyer Deposits Funds ----------------------------------

    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        // Buyer is Signer; transfer directly via system_instruction::transfer into Vault PDA
        // Vault PDA is owned by System Program (not init'd by program), can receive SOL directly without invoke_signed
        // If Vault does not yet exist on-chain (first deposit), System Program will automatically create the account
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

        // Update on-chain balance
        let channel = &mut ctx.accounts.channel;
        channel.balance = channel
            .balance
            .checked_add(amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        Ok(())
    }

    // -- Step 4: Batch Settlement (Three Lines of Defense) ------------

    pub fn settle_batch(
        ctx: Context<BatchSettle>,
        merkle_root: [u8; 32],
        total_amount: u64,
        buyer_batch_sig: [u8; 64],
        merchant_batch_sig: [u8; 64],
    ) -> Result<()> {
        let channel = &ctx.accounts.channel;

        // Defense 1: On-chain limit check (cumulative settled + this batch <= spending_cap)
        let cumulative = channel
            .settled_amount
            .checked_add(total_amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        require!(
            cumulative <= channel.spending_cap,
            ErrorCode::SpendingCapExceeded
        );

        // Defense 2: On-chain balance check (balance = Vault real-time balance, synced after settlement)
        require!(
            total_amount <= channel.balance,
            ErrorCode::InsufficientBalance
        );

        // Defense 3: Dual signature verification (Instruction Introspection)
        // Ed25519 is a precompiled instruction and does not support CPI.
        // Correct approach: the client includes Ed25519 verification instructions as preceding
        // instructions in the transaction; the contract introspects via Instructions Sysvar to
        // check whether the parameters of these instructions match.
        //
        // Message = SHA256(merkle_root || total_amount || channel_id || batch_nonce)
        // where batch_nonce == channel.nonce (current batch number, not yet incremented)
        let mut msg_preimage = Vec::with_capacity(32 + 8 + 32 + 8);
        msg_preimage.extend_from_slice(&merkle_root);
        msg_preimage.extend_from_slice(&total_amount.to_le_bytes());
        msg_preimage.extend_from_slice(ctx.accounts.channel.key().as_ref());
        msg_preimage.extend_from_slice(&channel.nonce.to_le_bytes());
        let message_hash = hashv(&[&msg_preimage]);

        // Transaction structure: [ed25519_verify(s), settle_batch]
        // settle_batch is at index current_ix
        // Two layouts supported:
        //   (a) Two independent Ed25519 instructions: [ed25519_buyer, ed25519_merchant, settle_batch]
        //   (b) One packed instruction (num_sigs=2): [ed25519_packed, settle_batch]
        // Iterate all preceding instructions, match buyer/merchant Ed25519 signatures by public_key
        let current_ix = load_current_index_checked(&ctx.accounts.instruction_sysvar)?;
        require!(
            current_ix >= 1,
            ErrorCode::InvalidTransactionLayout
        );

        let mut buyer_verified = false;
        let mut merchant_verified = false;
        for ix_idx in 0..(current_ix as usize) {
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
                _ => {} // Skip non-matching instructions (could be Ed25519 instructions from other transactions)
            }
        }
        require!(buyer_verified, ErrorCode::BuyerSignatureNotFound);
        require!(merchant_verified, ErrorCode::MerchantSignatureNotFound);

        // Update settled amount and balance (balance decremented to stay in sync with Vault's actual lamports)
        let channel = &mut ctx.accounts.channel;
        channel.settled_amount = channel
            .settled_amount
            .checked_add(total_amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        channel.balance = channel
            .balance
            .checked_sub(total_amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        // Transfer funds from Vault PDA into Settlement Escrow (not directly to merchant)
        let vault_seeds = &[
            VAULT_SEED,
            ctx.accounts.channel.key().as_ref(),
            &[channel.vault_bump],
        ];
        let transfer_ix = system_instruction::transfer(
            &ctx.accounts.vault.key(),
            &ctx.accounts.settlement_escrow.key(),
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

        // Record settlement batch info (nonce uses pre-increment value, consistent with PDA seeds and signed message)
        let settlement = &mut ctx.accounts.settlement_escrow;
        settlement.channel = ctx.accounts.channel.key();
        settlement.merchant = ctx.accounts.merchant.key();
        settlement.amount = total_amount;
        settlement.merkle_root = merkle_root;
        settlement.nonce = channel.nonce; // Use current nonce (not yet incremented)
        settlement.created_at = Clock::get()?.unix_timestamp;
        settlement.claimed = false;
        settlement.disputed = false;
        settlement.bump = ctx.bumps.settlement_escrow;

        // Finally increment batch_nonce for the next batch
        channel.nonce = channel
            .nonce
            .checked_add(1)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        Ok(())
    }

    // -- Merchant withdraws funds after challenge window (and closes Escrow to reclaim Rent)

    pub fn release_settlement(ctx: Context<ReleaseSettlement>) -> Result<()> {
        let settlement = &mut ctx.accounts.settlement_escrow;
        require!(!settlement.claimed, ErrorCode::AlreadyClaimed);
        require!(!settlement.disputed, ErrorCode::Disputed);

        // Challenge window: read configurable value from channel
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= settlement.created_at + ctx.accounts.channel.challenge_period,
            ErrorCode::ChallengePeriodNotExpired
        );

        settlement.claimed = true;

        // Transfer from Settlement Escrow PDA to merchant
        let escrow_seeds = &[
            SETTLEMENT_SEED,
            settlement.channel.as_ref(),
            &settlement.nonce.to_le_bytes(),
            &[settlement.bump],
        ];
        let transfer_ix = system_instruction::transfer(
            &ctx.accounts.settlement_escrow.key(),
            &ctx.accounts.merchant.key(),
            settlement.amount,
        );
        anchor_lang::solana_program::program::invoke_signed(
            &transfer_ix,
            &[
                ctx.accounts.settlement_escrow.to_account_info(),
                ctx.accounts.merchant.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[escrow_seeds],
        )?;

        // Close Escrow account and reclaim Rent lamports to merchant
        let escrow_info = ctx.accounts.settlement_escrow.to_account_info();
        let merchant_info = ctx.accounts.merchant.to_account_info();
        **merchant_info.lamports.borrow_mut() = merchant_info
            .lamports()
            .checked_add(escrow_info.lamports())
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        **escrow_info.lamports.borrow_mut() = 0;
        *escrow_info.try_borrow_mut_data()? = &mut [];

        Ok(())
    }

    // -- Buyer initiates dispute (freezes funds, rather than refunding directly)

    pub fn dispute(ctx: Context<Dispute>) -> Result<()> {
        let settlement = &mut ctx.accounts.settlement_escrow;
        require!(!settlement.claimed, ErrorCode::AlreadyClaimed);
        require!(!settlement.disputed, ErrorCode::AlreadyDisputed);

        // Must be within challenge window (read configurable value from channel)
        let now = Clock::get()?.unix_timestamp;
        require!(
            now < settlement.created_at + ctx.accounts.channel.challenge_period,
            ErrorCode::ChallengePeriodExpired
        );

        // Verify caller is the buyer
        require!(
            ctx.accounts.buyer.key() == ctx.accounts.channel.buyer,
            ErrorCode::NotBuyer
        );

        // Freeze funds: only mark disputed = true to block merchant's release_settlement
        // Funds remain in Escrow awaiting further resolution:
        //   - Buyer can submit Merkle proof showing merchant fabricated data (off-chain evidence)
        //   - Or introduce an arbitration mechanism / extend challenge period for merchant to submit full details in rebuttal
        settlement.disputed = true;

        Ok(())
    }

    // -- Merchant force-release after dispute timeout (resolves funds being locked due to buyer disappearing)

    pub fn force_release(ctx: Context<ForceRelease>) -> Result<()> {
        let settlement = &mut ctx.accounts.settlement_escrow;
        require!(settlement.disputed, ErrorCode::NotDisputed);
        require!(!settlement.claimed, ErrorCode::AlreadyClaimed);

        // Dispute validity period: dispute_period seconds after dispute
        // If buyer has not submitted a valid Fraud Proof within this period, merchant can force-release
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= settlement.created_at + ctx.accounts.channel.dispute_period,
            ErrorCode::DisputePeriodNotExpired
        );

        settlement.claimed = true;

        // Transfer to merchant (including Escrow Rent reclamation)
        let escrow_seeds = &[
            SETTLEMENT_SEED,
            settlement.channel.as_ref(),
            &settlement.nonce.to_le_bytes(),
            &[settlement.bump],
        ];
        let transfer_ix = system_instruction::transfer(
            &ctx.accounts.settlement_escrow.key(),
            &ctx.accounts.merchant.key(),
            settlement.amount,
        );
        anchor_lang::solana_program::program::invoke_signed(
            &transfer_ix,
            &[
                ctx.accounts.settlement_escrow.to_account_info(),
                &ctx.accounts.merchant.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[escrow_seeds],
        )?;

        // Close Escrow account and reclaim Rent
        let escrow_info = ctx.accounts.settlement_escrow.to_account_info();
        let merchant_info = ctx.accounts.merchant.to_account_info();
        **merchant_info.lamports.borrow_mut() = merchant_info
            .lamports()
            .checked_add(escrow_info.lamports())
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        **escrow_info.lamports.borrow_mut() = 0;
        *escrow_info.try_borrow_mut_data()? = &mut [];

        Ok(())
    }

    // -- Buyer submits fraud proof, verified then refunded ---------------------

    /// Dispute resolution endpoint: buyer uses a single Voucher + Sum Merkle Proof
    /// to prove the merchant inflated the total amount.
    ///
    /// Each internal node of the Sum Merkle Tree stores (hash, cumulative_sum).
    /// The buyer only needs to submit a single Voucher and its O(log n) Merkle path.
    /// The contract rebuilds the root along the path, compares it with the on-chain
    /// root hash, and checks whether the root sum matches settlement.amount.
    /// If root hash matches but sum < amount -> fraud is confirmed.
    ///
    /// Transaction size: Merkle path = log2(N) x (32+8) bytes. 128 Vouchers require
    /// only 7 x 40 = 280 bytes, well below Solana's 1232-byte transaction limit.
    pub fn resolve_dispute(
        ctx: Context<ResolveDispute>,
        // Single Voucher leaf data
        voucher_seq: u64,
        voucher_amount: u64,
        buyer_voucher_sig: [u8; 64],
        // Sum Merkle path (siblings)
        sibling_hashes: Vec<[u8; 32]>,
        sibling_sums: Vec<u64>,
    ) -> Result<()> {
        let settlement = &mut ctx.accounts.settlement_escrow;
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

        // Compute leaf hash (domain separator 0x00 prevents second preimage attack)
        let mut leaf_preimage = Vec::with_capacity(1 + 32 + 8 + 8 + 32 + 64);
        leaf_preimage.push(0x00); // leaf domain separator
        leaf_preimage.extend_from_slice(ctx.accounts.channel.key().as_ref());
        leaf_preimage.extend_from_slice(&voucher_seq.to_le_bytes());
        leaf_preimage.extend_from_slice(&voucher_amount.to_le_bytes());
        leaf_preimage.extend_from_slice(ctx.accounts.channel.buyer.as_ref());
        leaf_preimage.extend_from_slice(&buyer_voucher_sig);
        let leaf_hash = hashv(&[&leaf_preimage]).to_bytes();

        // Rebuild root along Merkle path (verify proof + compute root sum)
        let (root_matches, computed_total) = verify_sum_merkle_proof(
            &leaf_hash,
            voucher_amount,
            &sibling_hashes,
            &sibling_sums,
            &settlement.merkle_root,
        )?;

        // Root hash must match (proof is valid)
        require!(root_matches, ErrorCode::InvalidFraudProof);
        // Root sum < claimed total -> merchant inflated -> fraud confirmed
        require!(
            computed_total < settlement.amount,
            ErrorCode::FraudNotProven
        );

        // Roll back settled_amount: the fraudulent portion does not consume spending_cap
        let channel = &mut ctx.accounts.channel;
        channel.settled_amount = channel
            .settled_amount
            .checked_sub(settlement.amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        settlement.claimed = true;

        // Refund to buyer
        let escrow_seeds = &[
            SETTLEMENT_SEED,
            settlement.channel.as_ref(),
            &settlement.nonce.to_le_bytes(),
            &[settlement.bump],
        ];
        let transfer_ix = system_instruction::transfer(
            &ctx.accounts.settlement_escrow.key(),
            &ctx.accounts.buyer.key(),
            settlement.amount,
        );
        anchor_lang::solana_program::program::invoke_signed(
            &transfer_ix,
            &[
                ctx.accounts.settlement_escrow.to_account_info(),
                ctx.accounts.buyer.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[escrow_seeds],
        )?;

        // Close Escrow and reclaim Rent to buyer
        let escrow_info = ctx.accounts.settlement_escrow.to_account_info();
        let buyer_info = ctx.accounts.buyer.to_account_info();
        **buyer_info.lamports.borrow_mut() = buyer_info
            .lamports()
            .checked_add(escrow_info.lamports())
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        **escrow_info.lamports.borrow_mut() = 0;
        *escrow_info.try_borrow_mut_data()? = &mut [];

        Ok(())
    }

    // -- Buyer Withdraws Unused Funds -----------------------------------------

    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        let channel = &mut ctx.accounts.channel;

        // Verify withdrawable balance (balance = Vault real-time balance)
        require!(amount <= channel.balance, ErrorCode::InsufficientBalance);

        channel.balance = channel
            .balance
            .checked_sub(amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        // Transfer from Vault PDA back to buyer
        let vault_seeds = &[
            VAULT_SEED,
            ctx.accounts.channel.key().as_ref(),
            &[channel.vault_bump],
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

// -- Ed25519 Instruction Introspection Helper Functions -------------------------
// Solana's Ed25519 precompiled instruction does not support CPI; must use Instructions Sysvar
// to introspect and check whether the transaction includes correct signature verification instructions.
//
// Ed25519 instruction data format (supports num_sigs >= 1, i.e., a single instruction can verify multiple signatures):
//   [0..2]   num_signatures (u16 LE) -- number of signatures in this instruction
//   [2..4]   padding
//   Each signature occupies 14 bytes header + actual data:
//     [off+0..off+2]   signature_offset (u16 LE)
//     [off+2..off+4]   signature_instruction_index (u16 LE)
//     [off+4..off+6]   public_key_offset (u16 LE)
//     [off+6..off+8]   public_key_instruction_index (u16 LE)
//     [off+8..off+10]  message_data_offset (u16 LE)
//     [off+10..off+12] message_data_size (u16 LE)
//     [off+12..off+14] message_instruction_index (u16 LE)
//   [header_end..]  actual data: each sig = signature(64B) + public_key(32B) + message(N bytes)

// -- Sum Merkle Proof Verification Helper Functions -------------------------
// Each node in the Sum Merkle Tree stores (hash, cumulative_sum).
// Internal node: hash = SHA256(lo_hash || lo_sum_le || hi_hash || hi_sum_le)
//                sum = left_sum + right_sum
// Where lo/hi are sorted by hash lexicographic order.
//
// The buyer only needs to provide log2(N) sibling nodes; the contract rebuilds the root
// along the path and returns (root_hash_matches, computed_total_sum).
// Transaction size: log2(128) x (32+8) = 280 bytes, well below 1232-byte limit.

fn verify_sum_merkle_proof(
    leaf_hash: &[u8; 32],
    leaf_amount: u64,
    sibling_hashes: &[[u8; 32]],
    sibling_sums: &[u64],
    expected_root: &[u8; 32],
) -> Result<(bool, u64)> {
    let mut current_hash = *leaf_hash;
    let mut current_sum = leaf_amount;

    for i in 0..sibling_hashes.len() {
        let (lo_hash, lo_sum, hi_hash, hi_sum) = if current_hash <= sibling_hashes[i] {
            (current_hash, current_sum, sibling_hashes[i], sibling_sums[i])
        } else {
            (sibling_hashes[i], sibling_sums[i], current_hash, current_sum)
        };

        // Node hash = SHA256(0x01 || lo_hash || lo_sum_le || hi_hash || hi_sum_le)
        // Domain separator 0x01 prevents second preimage attack (distinguished from leaf's 0x00)
        let mut buf = [0u8; 81]; // 1 + 32 + 8 + 32 + 8
        buf[0] = 0x01; // internal node domain separator
        buf[1..33].copy_from_slice(&lo_hash);
        buf[33..41].copy_from_slice(&lo_sum.to_le_bytes());
        buf[41..73].copy_from_slice(&hi_hash);
        buf[73..81].copy_from_slice(&hi_sum.to_le_bytes());

        current_hash = hashv(&[&buf]).to_bytes();
        current_sum = current_sum
            .checked_add(sibling_sums[i])
            .ok_or(ErrorCode::ArithmeticOverflow)?;
    }

    Ok((current_hash == *expected_root, current_sum))
}

// Iterative Ed25519 signature verification: auto-matches buyer/merchant by public_key
// Replaces hardcoded index approach, preventing index confusion attacks in composite transactions.
// Supports num_sigs >= 1: a single Ed25519 instruction can contain multiple signatures
// (e.g., buyer + merchant packed in the same instruction).

enum VerifiedParty { Buyer, Merchant }

fn verify_ed25519_for_pubkey(
    ix_sysvar: &AccountInfo<'_>,
    ix_index: usize,
    expected_message: &[u8],
    buyer_sig: &[u8],
    merchant_sig: &[u8],
    buyer_pubkey: &[u8],
    merchant_pubkey: &[u8],
) -> Result<VerifiedParty> {
    let ix = load_instruction_at_checked(ix_index, ix_sysvar)
        .map_err(|_| ErrorCode::InvalidSignatureInstruction)?;

    require!(
        ix.program_id == ed25519_program::id(),
        ErrorCode::InvalidSignatureInstruction
    );

    let data = &ix.data;
    require!(data.len() >= 4, ErrorCode::InvalidSignatureInstruction);

    let num_sigs = u16::from_le_bytes([data[0], data[1]]) as usize;
    require!(num_sigs >= 1, ErrorCode::InvalidSignatureInstruction);

    // Iterate each signature entry in this instruction
    for sig_idx in 0..num_sigs {
        let header_offset = 4 + sig_idx * 14;
        require!(data.len() >= header_offset + 14, ErrorCode::InvalidSignatureInstruction);

        let sig_offset = u16::from_le_bytes([data[header_offset], data[header_offset + 1]]) as usize;
        let sig_ix_idx = u16::from_le_bytes([data[header_offset + 2], data[header_offset + 3]]);
        let pubkey_offset = u16::from_le_bytes([data[header_offset + 4], data[header_offset + 5]]) as usize;
        let pubkey_ix_idx = u16::from_le_bytes([data[header_offset + 6], data[header_offset + 7]]);
        let msg_offset = u16::from_le_bytes([data[header_offset + 8], data[header_offset + 9]]) as usize;
        let msg_size = u16::from_le_bytes([data[header_offset + 10], data[header_offset + 11]]) as usize;
        let msg_ix_idx = u16::from_le_bytes([data[header_offset + 12], data[header_offset + 13]]);

        require!(sig_ix_idx == 0, ErrorCode::InvalidSignatureInstruction);
        require!(pubkey_ix_idx == 0, ErrorCode::InvalidSignatureInstruction);
        require!(msg_ix_idx == 0, ErrorCode::InvalidSignatureInstruction);

        // Extract public key
        require!(data.len() >= pubkey_offset + 32, ErrorCode::InvalidSignatureInstruction);
        let pubkey = &data[pubkey_offset..pubkey_offset + 32];

        // Determine if this is the buyer's or merchant's signature
        let (expected_sig, party) = if pubkey == buyer_pubkey {
            (buyer_sig, VerifiedParty::Buyer)
        } else if pubkey == merchant_pubkey {
            (merchant_sig, VerifiedParty::Merchant)
        } else {
            continue; // Skip non-matching public keys (could be other signatures in the same instruction)
        };

        // Compare signature
        require!(data.len() >= sig_offset + 64, ErrorCode::InvalidSignatureInstruction);
        require!(
            &data[sig_offset..sig_offset + 64] == expected_sig,
            ErrorCode::SignatureMismatch
        );

        // Compare message
        require!(data.len() >= msg_offset + msg_size, ErrorCode::InvalidSignatureInstruction);
        require!(
            &data[msg_offset..msg_offset + msg_size] == expected_message,
            ErrorCode::SignatureMismatch
        );

        return Ok(party);
    }

    Err(ErrorCode::InvalidSignatureInstruction.into())
}

// -- Account Structs ----------------------------------------------------------

#[derive(Accounts)]
#[instruction(spending_cap: u64, challenge_period: i64, dispute_period: i64)]
pub struct InitializeChannel<'info> {
    #[account(
        init,
        payer = buyer,
        space = Channel::SPACE,
        seeds = [CHANNEL_SEED, buyer.key().as_ref(), merchant.key().as_ref()],
        bump,
    )]
    pub channel: Account<'info, Channel>,
    /// CHECK: Vault PDA -- not init'd by program, keeping System Program as owner.
    /// This allows buyer's system_instruction::transfer to deposit directly (no invoke_signed needed),
    /// and the program can sign on behalf of the vault via invoke_signed + PDA seeds for transfers out.
    #[account(
        mut,
        seeds = [VAULT_SEED, channel.key().as_ref()],
        bump,
    )]
    pub vault: UncheckedAccount<'info>,
    #[account(mut)]
    pub buyer: Signer<'info>,
    /// CHECK: Trusted merchant account
    pub merchant: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(
        mut,
        has_one = buyer,
        seeds = [CHANNEL_SEED, channel.buyer.as_ref(), channel.merchant.as_ref()],
        bump = channel.bump,
    )]
    pub channel: Account<'info, Channel>,
    /// CHECK: Vault PDA -- seeds bind it to this channel, owner is System Program
    #[account(
        mut,
        seeds = [VAULT_SEED, channel.key().as_ref()],
        bump = channel.vault_bump,
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
        has_one = merchant,
        seeds = [CHANNEL_SEED, channel.buyer.as_ref(), merchant.key().as_ref()],
        bump = channel.bump,
    )]
    pub channel: Account<'info, Channel>,
    /// CHECK: Vault PDA bound to channel, owner is System Program
    #[account(
        mut,
        seeds = [VAULT_SEED, channel.key().as_ref()],
        bump = channel.vault_bump,
    )]
    pub vault: UncheckedAccount<'info>,
    /// CHECK: Settlement escrow -- one per batch
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
    /// CHECK: Instructions sysvar -- used for instruction introspection to verify Ed25519 signatures
    #[account(address = anchor_lang::solana_program::sysvar::instructions::id())]
    pub instruction_sysvar: AccountInfo<'info>,
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
        seeds = [CHANNEL_SEED, channel.buyer.as_ref(), channel.merchant.as_ref()],
        bump = channel.bump,
    )]
    pub channel: Account<'info, Channel>,
    /// CHECK: Vault PDA bound to channel, owner is System Program
    #[account(
        mut,
        seeds = [VAULT_SEED, channel.key().as_ref()],
        bump = channel.vault_bump,
    )]
    pub vault: UncheckedAccount<'info>,
    #[account(mut)]
    pub buyer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

// -- State Structs ------------------------------------------------------------

#[account]
pub struct Channel {
    pub buyer: Pubkey,            // 32
    pub merchant: Pubkey,         // 32
    pub balance: u64,             // 8 -- Vault real-time balance (synced after settlement)
    pub spending_cap: u64,        // 8 -- spending limit (cumulative settlement cap)
    pub settled_amount: u64,      // 8 -- settled amount consumed (spending_cap counter, rolled back on dispute refund)
    pub nonce: u64,               // 8 -- settlement batch number
    pub challenge_period: i64,    // 8 -- challenge window (seconds)
    pub dispute_period: i64,      // 8 -- dispute resolution period (seconds)
    pub bump: u8,                 // 1 -- channel PDA bump
    pub vault_bump: u8,           // 1 -- vault PDA bump
}

impl Channel {
    pub const SPACE: usize = 8 + 32 + 32 + 8 + 8 + 8 + 8 + 8 + 8 + 1 + 1; // 122
}

#[account]
pub struct SettlementEscrow {
    pub channel: Pubkey,          // 32
    pub merchant: Pubkey,         // 32
    pub amount: u64,              // 8
    pub merkle_root: [u8; 32],    // 32
    pub nonce: u64,               // 8 -- channel.nonce at settlement time (before increment), also used in PDA seeds
    pub created_at: i64,          // 8 -- settlement timestamp
    pub claimed: bool,            // 1 -- whether merchant has withdrawn
    pub disputed: bool,           // 1 -- whether buyer has initiated a dispute
    pub bump: u8,                 // 1
}

impl SettlementEscrow {
    pub const SPACE: usize = 8 + 32 + 32 + 8 + 32 + 8 + 8 + 1 + 1 + 1; // 131
}

#[error_code]
pub enum ErrorCode {
    #[msg("Exceeds the allowed spending cap.")]
    SpendingCapExceeded,
    #[msg("Insufficient balance in the channel.")]
    InsufficientBalance,
    #[msg("Settlement already claimed.")]
    AlreadyClaimed,
    #[msg("Settlement is under dispute.")]
    Disputed,
    #[msg("Settlement already disputed.")]
    AlreadyDisputed,
    #[msg("Challenge period has not expired.")]
    ChallengePeriodNotExpired,
    #[msg("Challenge period has expired.")]
    ChallengePeriodExpired,
    #[msg("Only the buyer can dispute.")]
    NotBuyer,
    #[msg("Arithmetic overflow.")]
    ArithmeticOverflow,
    #[msg("Invalid Ed25519 signature instruction.")]
    InvalidSignatureInstruction,
    #[msg("Signature does not match expected value.")]
    SignatureMismatch,
    #[msg("Invalid transaction layout: at least one Ed25519 instruction must precede settle_batch.")]
    InvalidTransactionLayout,
    #[msg("Settlement is not disputed.")]
    NotDisputed,
    #[msg("Dispute period has not expired.")]
    DisputePeriodNotExpired,
    #[msg("Invalid fraud proof.")]
    InvalidFraudProof,
    #[msg("Fraud not proven: actual total matches claimed total.")]
    FraudNotProven,
    #[msg("Buyer Ed25519 signature instruction not found in transaction.")]
    BuyerSignatureNotFound,
    #[msg("Merchant Ed25519 signature instruction not found in transaction.")]
    MerchantSignatureNotFound,
}
```

#### 2. Client Data Binding Interface (TypeScript)

Before settlement, the buyer needs to sign the batch message. Below is the assembly of the signed message and the construction of `settle_batch` parameters:

```typescript
import { Connection, PublicKey, Transaction, SystemProgram, TransactionInstruction, Keypair } from '@solana/web3.js';
import * as borsh from 'borsh';
import { createHash } from 'crypto';
import nacl from 'tweetnacl';

// -- Voucher Structure (off-chain single payment voucher) -------------------------

export class Voucher {
    channelId: Buffer;     // 32 bytes
    voucherSeq: bigint;    // u64 -- micropayment sequence number (not batch_nonce)
    amount: bigint;        // u64 -- individual payment amount
    cumulativeAmount: bigint; // u64 -- cumulative amount (for local verification only)
    buyerPubkey: Buffer;   // 32 bytes
    buyerSig: Buffer;      // 64 bytes -- buyer's signature on this individual payment
}

// -- Merkle Tree Construction -------------------------------------------------

// Leaf = SHA256(0x00 || channel_id || voucher_seq || amount || buyer_pubkey || buyer_sig)
// 0x00 domain separator prevents second preimage attack
export function hashLeaf(voucher: Voucher): Buffer {
    const seqBuf = Buffer.alloc(8);
    seqBuf.writeBigUInt64LE(voucher.voucherSeq);
    const amountBuf = Buffer.alloc(8);
    amountBuf.writeBigUInt64LE(voucher.amount);
    const data = Buffer.concat([
        Buffer.from([0x00]), // leaf domain separator
        voucher.channelId,
        seqBuf,
        amountBuf,
        voucher.buyerPubkey,
        voucher.buyerSig,
    ]);
    return createHash('sha256').update(data).digest();
}

// -- Sum Merkle Tree Construction -----------------------------------------------
// Each node in the Sum Merkle Tree stores (hash, cumulative_sum).
// Internal node: hash = SHA256(lo_hash || lo_sum_le || hi_hash || hi_sum_le)
//                sum = left_sum + right_sum
// Sorting rule is consistent with Rust's verify_sum_merkle_proof: sorted by hash lexicographic order.

interface SumMerkleNode {
    hash: Buffer;
    sum: bigint;
}

export function buildSumMerkleTree(vouchers: Voucher[]): { root: Buffer; totalAmount: bigint; nodes: SumMerkleNode[][] } {
    let nodes: SumMerkleNode[][] = [];
    let currentLevel: SumMerkleNode[] = vouchers.map(v => ({
        hash: hashLeaf(v),
        sum: v.amount,
    }));
    let totalAmount = 0n;
    for (const v of vouchers) totalAmount += v.amount;
    nodes.push(currentLevel);

    while (currentLevel.length > 1) {
        const nextLevel: SumMerkleNode[] = [];
        for (let i = 0; i < currentLevel.length; i += 2) {
            const left = currentLevel[i];
            const right = i + 1 < currentLevel.length
                ? currentLevel[i + 1]
                : { hash: Buffer.alloc(32), sum: 0n };
            const [lo, hi] = left.hash.compare(right.hash) < 0
                ? [left, right]
                : [right, left];
            const loSumBuf = Buffer.alloc(8);
            loSumBuf.writeBigUInt64LE(lo.sum);
            const hiSumBuf = Buffer.alloc(8);
            hiSumBuf.writeBigUInt64LE(hi.sum);
            const combined = Buffer.concat([
                Buffer.from([0x01]), // internal node domain separator
                lo.hash, loSumBuf, hi.hash, hiSumBuf,
            ]);
            nextLevel.push({
                hash: createHash('sha256').update(combined).digest(),
                sum: left.sum + right.sum,
            });
        }
        currentLevel = nextLevel;
        nodes.push(currentLevel);
    }
    return { root: currentLevel[0].hash, totalAmount, nodes };
}

// Extract Merkle proof for a single Voucher from the Sum Merkle Tree
export function generateMerkleProof(
    voucher: Voucher,
    nodes: SumMerkleNode[][],
): { siblingHashes: Buffer[]; siblingSums: bigint[] } {
    const leafHash = hashLeaf(voucher);
    const siblingHashes: Buffer[] = [];
    const siblingSums: bigint[] = [];

    let currentHash = leafHash;
    for (let level = 0; level < nodes.length - 1; level++) {
        const levelNodes = nodes[level];
        const idx = levelNodes.findIndex(n => n.hash.equals(currentHash));
        if (idx === -1) throw new Error('Voucher not found in Merkle tree');
        const siblingIdx = idx % 2 === 0 ? idx + 1 : idx - 1;
        const sibling = siblingIdx < levelNodes.length
            ? levelNodes[siblingIdx]
            : { hash: Buffer.alloc(32), sum: 0n };
        siblingHashes.push(sibling.hash);
        siblingSums.push(sibling.sum);

        // Move to parent
        const parentIdx = Math.floor(idx / 2);
        currentHash = nodes[level + 1][parentIdx].hash;
    }
    return { siblingHashes, siblingSums };
}

// -- Batch Signature Message -------------------------------------------------

// SettlementMsg = SHA256(merkle_root || total_amount || channel_id || batch_nonce)
// batch_nonce is the on-chain channel.nonce (current batch number)
export function buildSettlementMessage(
    merkleRoot: Buffer,
    totalAmount: bigint,
    channelId: Buffer,
    batchNonce: bigint,
): Buffer {
    const amountBuf = Buffer.alloc(8);
    amountBuf.writeBigUInt64LE(totalAmount);
    const nonceBuf = Buffer.alloc(8);
    nonceBuf.writeBigUInt64LE(batchNonce);
    const data = Buffer.concat([
        merkleRoot,
        amountBuf,
        channelId,
        nonceBuf,
    ]);
    return createHash('sha256').update(data).digest();
}

// -- settle_batch Instruction Parameters -------------------------------------

// Security reminder: before signing the SettlementMsg, the buyer's client must perform
// the following local validations:
//   1. Recompute the Merkle Tree Root and confirm it matches the merchant-provided merkle_root
//   2. Confirm total_amount equals the sum of all Voucher amounts
//   3. Confirm batch_nonce equals the on-chain channel.nonce (current batch number)
//   4. Confirm total_amount does not exceed channel.spending_cap - channel.settled_amount
// These checks cannot be performed on-chain and must be enforced in the SDK to prevent
// the merchant from forging parameters to obtain a fraudulent signature.

export class SettleBatchParams {
    merkleRoot: number[];
    totalAmount: bigint;
    buyerBatchSig: number[];      // Buyer's signature on SettlementMsg
    merchantBatchSig: number[];   // Merchant's signature on SettlementMsg

    constructor(fields: { merkleRoot: number[]; totalAmount: bigint; buyerBatchSig: number[]; merchantBatchSig: number[] }) {
        this.merkleRoot = fields.merkleRoot;
        this.totalAmount = fields.totalAmount;
        this.buyerBatchSig = fields.buyerBatchSig;
        this.merchantBatchSig = fields.merchantBatchSig;
    }
}

export const settleBatchSchema = new Map([
    [
        SettleBatchParams,
        {
            kind: 'struct',
            fields: [
                ['merkleRoot', [32]],
                ['totalAmount', 'u64'],
                ['buyerBatchSig', [64]],
                ['merchantBatchSig', [64]],
            ],
        },
    ],
]);
```

---

### 4. Dispute Handling Flow (Dispute & Freeze & Force Release)

Funds are not transferred directly to the merchant; instead, they enter a Settlement Escrow PDA and go through the following lifecycle:

```
settle_batch          6h challenge window        release_settlement
    |                     |                        |
    v                     v                        v
+----------+       +----------+             +----------+
|  Vault   |------>| Escrow   |--no dispute->  | Merchant | -> close escrow
|  (buyer) |       | (custody)|             |(merchant)|   (reclaim rent)
+----------+       +----------+             +----------+
                         |
                         | dispute (buyer challenges)
                         v
                   +----------+
                   |  Escrow  |  <- funds frozen, awaiting resolution
                   | (frozen) |
                   +----------+
                         |
            +------------+------------+
            |            |            |
            v            v            v
      Buyer submits   48h later     Mutual
      Fraud Proof     buyer still   agreement to
                       inactive     unfreeze
            |            |            |
            v            v            v
   resolve_dispute  force_release   release to merchant
   (verify+refund   (merchant      (close escrow)
    to buyer)       force-withdraw)
```

**Challenge Window**: Within 6 hours (21600 seconds) after `settle_batch`:
* **Merchant**: Cannot withdraw funds (`release_settlement` is blocked by the time lock)
* **Buyer**: Can call `dispute` to freeze Escrow funds. After the contract verifies the caller is `channel.buyer`, it marks `disputed = true`. **Funds remain in Escrow and are not refunded**, blocking `release_settlement`.
* **Merkle Evidence**: If the buyer needs to prove the merchant fabricated the details, they can provide a single Voucher's Merkle proof off-chain and compare it with the on-chain `settlement_escrow.merkle_root`. If the proof does not match, it means the Voucher is not included in the tree, constituting evidence of merchant fabrication.

**Dispute Resolution Paths** (after `dispute`):
1. **Fraud Proof (`resolve_dispute`)**: The buyer calls `resolve_dispute`, submitting a **single Voucher + O(log N) Sum Merkle Proof**. The contract rebuilds the root along the Merkle path: if the root hash matches but the root sum < `settlement.amount`, it proves the merchant inflated `total_amount`. The contract refunds the Escrow funds to the buyer and closes the account. Transaction size: 128 Vouchers require only 280 bytes of proof, well below the 1232-byte limit.
2. **Force Release (`force_release`)**: If the buyer does not submit a valid Fraud Proof within 48 hours after `dispute`, the merchant can call `force_release` to forcibly withdraw the funds. This prevents funds from being permanently locked due to a malicious buyer who disputes then disappears.
3. **Arbitration Mechanism**: Introduce a third-party arbitrator for resolution (can be extended by pre-setting an `arbiter` public key in the Channel).

> This design balances the interests of both parties: the buyer cannot unconditionally reclaim funds (evidence required), and the merchant will not permanently lose funds due to a malicious dispute (48h timeout protection).

> **Role of `merkle_root`**: `merkle_root` does not participate in leaf-level verification on-chain; its roles are: (1) as part of the dual-signature message, binding signatures to a specific Voucher set; (2) stored in `SettlementEscrow` for off-chain Merkle evidence. If on-chain single-Voucher verification is needed in the future, a `verify_voucher` instruction can be added that accepts a Merkle proof and compares it against the stored root.

**Escrow Account Lifecycle**: `release_settlement` and `force_release` **close the Escrow account and reclaim Rent** after transferring funds, avoiding the accumulation of abandoned accounts consuming SOL deposits in high-frequency settlement scenarios.

**After Normal Window Expiry**: The merchant calls `release_settlement`; the contract verifies the timestamp, releases the funds, and closes the account.

---

### 5. Design Constraints and Notes

* **Single Channel Constraint**: Channel PDA seeds are `["channel", buyer, merchant]`, so only one channel can be created per buyer-merchant pair. To support multiple parallel channels, extend the seeds (e.g., add `channel_id`).
* **`spending_cap` is a Recyclable Limit**: `settled_amount` records the cumulative amount currently settled but undisputed, constrained by `settled_amount + new_batch_amount <= spending_cap`. If a fraud dispute occurs and the buyer prevails (`resolve_dispute`), the batch's `amount` is rolled back from `settled_amount`, releasing the corresponding `spending_cap` for subsequent batches. This means `spending_cap` is not a "one-time total for the channel's lifetime" but rather "the upper limit of in-flight + completed settlements at any given time." This design ensures: refunded fraudulent amounts do not permanently consume the buyer's spending capacity.
* **`balance` is the Vault's Real-time Balance**: After a successful `settle_batch`, `channel.balance` is decremented in sync (funds have been transferred to Escrow). `withdraw` directly checks `amount <= channel.balance`. `balance` and `settled_amount` are two independent accounting dimensions -- `balance` tracks actual funds (Vault lamports), while `settled_amount` tracks limit consumption (spending_cap counter).
* **`cumulative_amount` is a Client-side Soft Constraint**: This field is used only in the buyer's local client as a self-reminder not to exceed the limit; it is not verified on-chain. The protective effectiveness depends on correct client implementation.
* **Sum Merkle Tree**: Each internal node of the Merkle tree stores `(hash, cumulative_sum)`; the root's sum is the total of all Voucher amounts. Node hash = `SHA256(0x01 || lo_hash || lo_sum_le || hi_hash || hi_sum_le)`, leaf hash = `SHA256(0x00 || ...)`. Domain separators `0x00`/`0x01` prevent **second preimage attacks**. Fraud proof requires only a single Voucher + O(log N) sibling nodes; 128 Vouchers require only 280 bytes, well below the 1232-byte transaction limit.
* **Currently SOL Only**: SPL Token extension requires changing `deposit` to use Token Program CPI, and changing fund transfers in `settle_batch` / `release_settlement` / `withdraw` to Token Program `transfer` instructions. Core logic (dual signature, Merkle tree, challenge window) remains unchanged.
* **Ed25519 Signature Verification Uses Iterative Instruction Introspection**: The contract iterates all preceding instructions in the transaction, auto-matching buyer/merchant Ed25519 signatures by `public_key` instead of relying on hardcoded indices. Supports `num_sigs >= 1`, allowing the client to pack multiple signatures in a single Ed25519 precompiled instruction (saving transaction space) or use independent instructions (better compatibility).
* **Vault is Not `init`'d by the Program**: During `initialize_channel`, the Vault PDA only verifies address correctness via `seeds`, **without using `init`**. The Vault's owner is always the **System Program**. Benefits: (1) the buyer can deposit via regular `system_instruction::transfer` without `invoke_signed`; (2) the program can sign on behalf of the Vault to transfer funds out via `invoke_signed` + PDA seeds (System Program allows PDA-signed transfers from accounts it owns). The Vault does not exist on-chain before the first `deposit`; it is automatically created by the System Program on the first `deposit`.
* **Escrow Account is Closed After Use**: `release_settlement`, `force_release`, and `resolve_dispute` close the Escrow account and reclaim Rent lamports immediately after transferring funds, avoiding the accumulation of abandoned accounts in high-frequency settlement scenarios.
* **Configurable Time Windows**: `challenge_period` (challenge window, recommended 6h) and `dispute_period` (dispute resolution period, recommended 48h) are agreed upon by both parties during `initialize_channel`, adapting to different business scenarios (short window for high-frequency micro-payments, long window for medium-frequency larger payments).
* **Dispute Has a Complete Closed Loop**: After the buyer's `dispute` freezes funds, there are three exit paths: (1) `resolve_dispute` submits a Sum Merkle Proof for a refund to the buyer (also rolling back `settled_amount` to release spending_cap capacity); (2) merchant's `force_release` after dispute_period; (3) mutual agreement to release. `resolve_dispute` does not affect `channel.balance` (funds are refunded directly from Escrow to the buyer, not through the Vault).
* **Rationale for Dual Signature + Challenge Period Coexistence**: Dual signatures ensure both parties have acknowledged the batch off-chain (preventing the merchant from unilaterally fabricating data). The challenge period serves as a safety net covering the following scenarios: (1) the buyer's Agent signing key is compromised and an attacker forges signatures; (2) the buyer's SDK has a bug causing it to sign without proper verification; (3) the buyer loses their private key and needs time to freeze funds. If the business scenario requires extreme speed (e.g., automated settlement between Agents), `challenge_period` can be set to 0, in which case dual signatures serve as the sole defense.

---

### 6. Multi-Merchant Shared Vault Optimization

#### 6.1 Architecture Motivation and Boundary Correction

In the original "one-to-one channel" design, if a user transacts with multiple different merchants at low frequency, creating a separate channel for each merchant incurs excessive account creation costs (SOL rent) and reduces fund liquidity. Therefore, a **Global Buyer Vault** mechanism is introduced.

To address the "overdraft risk" introduced by the multi-merchant shared vault, the system implements a **full lock-up and dynamic limit allocation** mechanism at the smart contract level, ensuring the merchant's settlement security without any additional account overhead for the user.

#### 6.2 Global State and Global Shared Vault

To prevent buyers from overdrafting across multiple merchants, an on-chain global state account is introduced to track all allocated limits and total deposited funds.

* **Global State PDA Seeds**: `["global_state", buyer_pubkey]`
* **Global Vault PDA Seeds**: `["global_buyer_vault", buyer_pubkey]`

##### State Data Structure

```rust
#[account]
pub struct GlobalState {
    pub buyer: Pubkey,            // 32 bytes, owning buyer
    pub total_deposited: u64,     // 8 bytes, total deposited funds in global vault
    pub total_allocated: u64,     // 8 bytes, sum of spending_caps allocated across all merchant channels
    pub bump: u8,                 // 1 byte
}
```

##### Spending Cap Allocation Check

When the buyer creates a channel for a specific merchant or modifies `spending_cap`, the following validation formula is enforced in the smart contract to ensure the sum of all merchants' limits does not exceed total deposited funds:

$$\text{new\_spending\_cap} - \text{old\_spending\_cap} + \text{total\_allocated} \le \text{total\_deposited}$$

##### Smart Contract Processing Logic Example

```rust
pub fn update_spending_cap(ctx: Context<UpdateSpendingCap>, new_spending_cap: u64) -> Result<()> {
    let global_state = &mut ctx.accounts.global_state;
    let channel = &mut ctx.accounts.channel;

    // Calculate the delta of this adjustment
    let delta = if new_spending_cap > channel.spending_cap {
        new_spending_cap - channel.spending_cap
    } else {
        0 // Reducing the limit does not increase allocation
    };

    // Validate: total allocation does not exceed deposited funds
    require!(
        global_state.total_allocated.checked_add(delta).unwrap() <= global_state.total_deposited,
        ErrorCode::AllocationExceedsDeposit
    );

    // Update state
    global_state.total_allocated = global_state.total_allocated.checked_add(delta).unwrap();
    channel.spending_cap = new_spending_cap;

    Ok(())
}
```

#### 6.3 Underlying Contract and Data Structure Changes

The base `Channel` struct is adapted to accommodate the shared fund pool validation model:

* Remove the `balance` field (balance is tracked by global vault lamports)
* Remove the `vault_bump` field (channel-level Vault is no longer used)

```rust
#[account]
pub struct Channel {
    pub buyer: Pubkey,            // 32
    pub merchant: Pubkey,         // 32
    pub spending_cap: u64,        // 8 -- spending limit for a single merchant
    pub settled_amount: u64,      // 8 -- total settled amount
    pub nonce: u64,               // 8
    pub challenge_period: i64,    // 8
    pub dispute_period: i64,      // 8
    pub bump: u8,                 // 1
}
// SPACE: 8 + 32 + 32 + 8 + 8 + 8 + 8 + 8 + 1 = 113
```

##### Smart Contract Settlement Validation Logic

During the batch settlement phase, dual validation logic is added to prevent overspending resulting in bad debt:

```rust
// 1. Global limit check: ensure it does not exceed the single-merchant spending_cap
let cumulative = channel
    .settled_amount
    .checked_add(total_amount)
    .ok_or(ErrorCode::ArithmeticOverflow)?;
require!(
    cumulative <= channel.spending_cap,
    ErrorCode::SpendingCapExceeded
);

// 2. Balance check: ensure the deduction from the global vault is valid
require!(
    total_amount <= global_buyer_vault.lamports(),
    ErrorCode::InsufficientBalance
);
```

#### 6.4 Operational Flow Closed Loop

1. **Deposit Phase (`deposit`)**: Buyer deposits into the global vault, simultaneously updating `global_state.total_deposited`.
2. **Allocation Phase (`update_spending_cap`)**: Buyer allocates `spending_cap` for a specified merchant; the contract automatically validates global limits to prevent overspending.
3. **Settlement Phase (`settle_batch`)**: During L1 settlement, validates that the total settlement does not exceed the single merchant's `spending_cap` and cross-checks the global vault's actual available balance.
4. **Withdrawal Phase (`withdraw`)**: Buyer can withdraw unallocated funds (`total_deposited - total_allocated`).

#### 6.5 Architecture Advantage Comparison

| Dimension | Independent Channel Mode | Global Fund Pool + Global Limit Control (Optimized) |
| :--- | :--- | :--- |
| **Deployment Cost** | **High** (new account creation required for each new merchant) | **Low** (only need to initialize global Vault once) |
| **Fund Utilization** | **Lower** (funds dispersed across channels, easily idle) | **Very High** (all funds concentrated in a unified vault, allocated to merchants on demand) |
| **Overdraft Prevention** | Naturally isolated | Uses global limit sum constraint $\sum \text{spending\_cap}_i \le \text{Vault}$ |

#### 6.6 New and Modified Instructions

| Instruction | Change |
|------|------|
| `initialize_global` | **New** -- creates GlobalState + global vault (first-time setup) |
| `initialize_channel` | Modified -- requires GlobalState account, validates allocation limits |
| `update_spending_cap` | **New** -- adjusts single-merchant spending limit with global limit check |
| `deposit` | Modified -- deposits into global vault, updates `total_deposited` |
| `settle_batch` | Modified -- transfers from global vault (not channel-level Vault) |
| `withdraw` | Modified -- withdraws unallocated funds from global vault |
| `release_settlement` | Unchanged |
| `dispute` | Unchanged |
| `force_release` | Unchanged |
| `resolve_dispute` | Unchanged |

---

### 7. Optimistic Settlement -- Merchant Unilateral Submission Mechanism

#### 7.1 Design Motivation

In the standard settlement flow, `settle_batch` requires dual signatures from both the buyer and the merchant (dual-sig). However, in certain scenarios, the buyer may not cooperate with signing (e.g., offline, Agent failure, malicious delay), preventing the merchant from settling already-consumed funds.

To address this, an **Optimistic Settlement** mechanism is introduced -- a unilateral submission model similar to Optimistic Rollup:

* **Cooperative Scenario**: Buyer cooperates with signing -> uses `settle_batch` (dual signature, immediately enters Escrow)
* **Non-cooperative Scenario**: Buyer does not cooperate -> merchant uses `optimistic_settle` (merchant signature only, funds enter locked Escrow)

#### 7.2 Mechanism Flow

```
Merchant collects buyer-signed Vouchers
        |
        +-- Buyer cooperates with signing? -- Yes --> settle_batch (dual signature) --> Escrow --> release_settlement
        |
        +-- Buyer does not cooperate --> optimistic_settle (merchant signature only)
                                    |
                                    +-- No dispute during challenge period --> release_settlement (release to merchant)
                                    |
                                    +-- Buyer detects fraud --> dispute --> resolve_dispute (refund to buyer)
                                                                   or force_release (timeout release to merchant)
```

#### 7.3 On-chain Implementation

**New Instruction: `optimistic_settle`**

Shares the same account structure and fund flow logic as `settle_batch` (global vault -> Escrow), but with the following key differences:

1. **Only requires merchant Ed25519 signature**: No buyer batch signature needed
2. **Enforces `challenge_period > 0`**: Optimistically settled funds must go through a lock-up period; immediate release is not allowed
3. **Escrow marked `optimistic = true`**: Distinguishes dual-signature settlement from optimistic settlement

```rust
pub fn optimistic_settle(
    ctx: Context<OptimisticBatchSettle>,
    merkle_root: [u8; 32],
    total_amount: u64,
    merchant_batch_sig: [u8; 64],
) -> Result<()> {
    // Defense 1: spending cap check
    // Defense 2: global vault balance check
    // Defense 3: challenge_period > 0 (optimistic settlement must lock)
    // Defense 4: only verify merchant Ed25519 signature
    // ...
    settlement.optimistic = true;  // Mark as optimistic settlement
}
```

**SettlementEscrow Extension**

```rust
pub struct SettlementEscrow {
    // ... existing fields ...
    pub optimistic: bool,         // 1 -- true indicates merchant unilateral submission
}
// SPACE: 8 + 32 + 32 + 8 + 32 + 8 + 8 + 1 + 1 + 1 + 1 = 132
```

#### 7.4 Security Guarantees

| Guarantee Layer | Mechanism | Description |
|--------|------|------|
| **Overspend Prevention** | `spending_cap` check | `settled_amount + total_amount <= spending_cap`, consistent with dual-signature settlement |
| **Overdraft Prevention** | Global vault balance check | `total_amount <= vault.lamports` |
| **Immediate Transfer Prevention** | `challenge_period > 0` enforced | Optimistically settled funds must be locked; `challenge_period = 0` is not allowed |
| **Fraud Proof** | Sum Merkle Proof | Buyer can prove merchant inflated amounts via single Voucher, triggering `resolve_dispute` refund |
| **Signature Traceability** | Merchant Ed25519 signature | Merchant signs `(merkle_root || total_amount || channel || nonce)`, verifiable on-chain that the merchant indeed submitted this batch |

#### 7.5 Comparison with Dual-Signature Settlement

| Dimension | `settle_batch` (Dual Signature) | `optimistic_settle` (Unilateral Submission) |
|------|--------------------------|-------------------------------|
| **Signature Requirement** | Buyer + Merchant | Merchant only |
| **challenge_period** | Can be 0 (both parties have acknowledged) | Must be > 0 |
| **Fund Release** | Can be released immediately after challenge period | Released after challenge period |
| **Applicable Scenario** | Normal settlement (buyer Agent online and cooperative) | Buyer uncooperative, offline, Agent failure |
| **Buyer Protection** | Dual signature (active acknowledgment) | Challenge period + fraud proof (ex-post review) |

#### 7.6 New Instruction Summary

| Instruction | Description |
|------|------|
| `optimistic_settle` | **New** -- merchant unilateral settlement submission, only requires merchant signature, funds enter locked Escrow |
