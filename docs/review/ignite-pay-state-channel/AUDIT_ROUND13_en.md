# Ignite Pay State Channel — Round 13 Audit Report

> Review basis: `docs/utxo_merkletree_state_channel.md` (design specification)
> Review scope: `ignite-pay-state-channel/` project + `ignite-pay-program/` on-chain program
> Review date: 2026-04-11 (Round 13)
> Prior status: All 20 issues from Round 12 have been fixed, all 161 tests passing

---

## Audit Overview

The thirteenth round of audit identified a total of **50** issues:

| Category | P0 | P1 | P2 | P3 | Total |
|----------|----|----|----|----|-------|
| BUG      | 3  | 6  | 12 | 2  | 23    |
| DEV      | -  | 3  | 9  | 4  | 16    |
| PROG     | -  | 1  | 3  | 1  | 5     |
| TEST     | -  | -  | -  | 6  | 6     |
| **Total** | **3** | **10** | **24** | **13** | **50** |

---

## I. Off-Chain Code Audit (`ignite-pay-state-channel/`)

### BUG-22: `claim_leaf` does not reject HTLC-type leaves, allowing bypass of hash-lock verification (P1 — Fund Security)

**File**: `src/channel.rs:959-1061`

The `claim_leaf` method does not verify whether the leaf type is `Standard`. Per design document §5.2, Claim is intended for Standard leaves; HTLC leaves must use VerifyHTLC or HTLCRefund.

The test `test_claim_leaf_and_htlc_verify_exclusive` (channel_tests.rs:1246) also acknowledges this in its comment: "even though it's HTLC, claim_leaf doesn't check leaf_type".

**Impact**: The owner of an HTLC leaf can bypass hash-lock/preimage verification via `claim_leaf` and directly claim HTLC funds.

**Fix**: Add `if leaf.leaf_type != LeafType::Standard { return Err(...) }` in `claim_leaf`.

---

### BUG-23: `trigger_challenge` signature uses `current_slot` instead of `submitted_sequence` (P1 — Security)

**File**: `src/channel.rs:820-823`

```rust
let message = crate::signing::state_message(
    &state.metadata.channel_id,
    current_slot,        // BUG: should be submitted_sequence
    submitted_root,
);
```

Design document §4.3 specifies the state signature format as `SHA-256(channel_id || sequence || root)`. The signature should bind to the submitted sequence number, not the current slot.

**Impact**:
1. The signature is not bound to the submitted state version, making it impossible to verify the sequence intended by the challenger
2. The same root/sequence requires different signatures under different slots, which is semantically incorrect

**Fix**: Change `current_slot` to `submitted_sequence`.

---

### BUG-24: `partial_transfer` / `split_from_rest` operation order is reversed compared to the design document (P2)

**File**: `src/pipeline.rs:130-155`, `src/helpers.rs:63-93`

The code first creates the target leaf (increasing the total), then deducts from the source leaf (restoring the total). Design document §3.2.2 explicitly states: "Must first deduct from Rest, then create the target. This order guarantees that the amount conservation invariant is not broken in any intermediate state. If the order is reversed, it creates money out of thin air in the intermediate state."

At `seq1` (target creation), the total temporarily exceeds `total_deposited` by `amount`. If an on-chain challenge is triggered between `seq1` and `seq2`, the Merkle root will show `total > deposited`, which is a conservation violation that can be disputed.

The code comments acknowledge this deviation and argue for a "relaxed invariant `sum >= total_deposited`", but this is inconsistent with the design document's strict invariant `sum == total_deposited`.

---

### BUG-25: `merge_spent_leaves` uses `+` instead of `checked_add`, causing overflow (P2)

**File**: `src/helpers.rs:132`, `src/helpers.rs:157`

Two places in `merge_spent_leaves` use `+` instead of `saturating_add` or `checked_add`:

```rust
// Line 132: accumulating source leaves' amounts
total_amount += leaf.amount;  // may overflow u64

// Line 157: target.amount + total_amount
let new_target = UTXOLeaf::standard(target_owner, target_leaf.amount + total_amount);  // may overflow
```

If the sum of multiple leaf amounts exceeds `u64::MAX`, it will cause an arithmetic overflow panic or incorrect amounts.

---

### BUG-26: `helpers.rs:81` `split_from_rest` uses `-` instead of `checked_sub` (P2)

**File**: `src/helpers.rs:81`

```rust
let updated_rest = UTXOLeaf::standard(rest_leaf.owner, rest_leaf.amount - amount);
```

Since it has already been verified above that `rest_leaf.amount >= amount`, this will not overflow here. However, for code consistency (to match the `saturating_sub` usage in `channel.rs`), it is recommended to use `checked_sub` or `saturating_sub`.

---

### BUG-27: `routing.rs:187` fee calculation integer division truncation (P2)

**File**: `src/routing.rs:187`

```rust
let fee = req.amount * metrics.fee_rate_bps as u64 / 10000;
```

When `req.amount * fee_rate_bps` overflows `u64` (e.g., `amount = u64::MAX`, `fee_rate_bps = 65535`), the multiplication result will wrap around. Should use `checked_mul` / `saturating_mul`.

---

### BUG-28: `routing.rs:194` liquidity check does not include fees (P2)

**File**: `src/routing.rs:194`

```rust
if metrics.available_liquidity < req.amount {
    sufficient_liquidity = false;
}
```

During actual routing, each hop requires `amount + fee` in liquidity (because upstream needs to forward the amount + fee). Design document §10.3.2 specifies the check should be `min_liquidity < amount + total_fee`. The check should be `metrics.available_liquidity < req.amount + total_fee_accumulated`.

---

### BUG-29: `compliance.rs:170` `record_payment` window calculation error when `slot=0` is passed (P2)

**File**: `src/channel.rs:409`, `src/compliance.rs:170`

`apply_leaf_update` passes `slot=0` when calling `record_payment`:
```rust
cm.record_payment(
    state.metadata.channel_id,
    update.new_leaf.amount,
    0, // slot not available
    ...
)?;
```

When `slot=0` and `window_slots=1000`, `slot < window_slots`, so pruning is skipped. This causes `window_payments` to grow indefinitely, and eventually `window_spend` will continuously exceed the threshold, triggering erroneous compliance holds.

---

### BUG-30: `compliance.rs:188` threshold check uses `window_spend` instead of `cumulative_spent` (P2)

**File**: `src/compliance.rs:188`

```rust
let action = if window_spend >= state.limits.threshold {
```

The threshold described in design document §10.7 should be a "cumulative spending" concept, not spending within a window. It needs to be clarified: is `threshold` a "sliding window threshold" or a "cumulative threshold"? The current implementation assumes "within-window threshold", but the field name `cumulative_spent` is misleading.

---

### BUG-31: `htlc.rs:create_htlc` timelock calculation overflow (P2)

**File**: `src/htlc.rs:133`

```rust
let timelock_slot = current_slot + duration_slots;
```

If `current_slot` is close to `u64::MAX`, the addition may overflow and wrap to a past time value. The `create_htlc` in Pipeline uses `saturating_add`, but `HtlcManager::create_htlc` uses plain `+`.

---

### BUG-32: `claim_htlc_verify` requires `settle_deadline` to be unset in Challenged state (P2)

**File**: `src/channel.rs:1190-1193`

When the channel is in `Challenged` state, `settle_deadline` may not be set (it is only set when transitioning to `Settling`). But the code unconditionally checks `settle_deadline`. Design document §4.2 specifies that VerifyHTLC is available in Challenged state but does not specify a `settle_deadline` check. Tests bypass this issue by manually setting `settle_deadline`.

---

### DEV-12: `helpers.rs:157` merge target equals source causes overwrite issue (P2)

**File**: `src/helpers.rs:157`

If `target_idx` appears in `source_indices` (e.g., merging `[0,1]` to target=0), then in Step 1 the target is first updated to `target.amount + total_amount`, but at this point `total_amount` already includes the target's own amount (since the target is also a source). When Step 2 clears the sources, it will clear the target as well, resulting in lost funds.

Missing validation that `target_idx` must not be in `source_indices`.

---

### DEV-13: `routing.rs:285` `select_best_route` assumes sorted input (P3)

**File**: `src/routing.rs:285`

```rust
pub fn select_best_route(routes: &[Route]) -> Option<&Route> {
    routes.first()
}
```

This method assumes `routes` is already sorted by score in descending order. Although `discover_routes` does return sorted results, if a user passes unsorted routes (e.g., an externally constructed route list), it will return an incorrect result. It is recommended to use `max_by_key` instead, or document the sorting prerequisite.

---

### DEV-14: `routing.rs:238` fee overflow in `score_route` (P2)

**File**: `src/routing.rs:238-240`

```rust
let total_fee: u64 = path_metrics.iter()
    .map(|m| amount * m.fee_rate_bps as u64 / 10000)
    .sum();
```

`.sum()` uses default `u64` addition with no overflow protection. Should use `saturating_add` or `checked_add`. Similarly, `amount * m.fee_rate_bps as u64` may also overflow.

---

### DEV-15: `signing.rs` signature message format is inconsistent with design document (P2)

**File**: `src/signing.rs:28-38`

Design document §4.3 specifies that `state_message` should return a 72-byte raw concatenation `[u8; 72]`:
```rust
fn state_message(channel_id: &[u8; 32], sequence: u64, root: &[u8; 32]) -> [u8; 72]
```

But the implementation returns a 32-byte hash of `SHA-256(channel_id || sequence || root)`. If the on-chain program uses the raw 72-byte format, off-chain signatures will fail on-chain verification. It is necessary to ensure the signature message format is consistent between off-chain and on-chain.

---

### DEV-16: `channel.rs:1441` `finalize_settlement` refund precision loss (P3)

**File**: `src/channel.rs:1441-1449`

```rust
let ratio_a = state.metadata.deposit_a as u128 * 1_000_000 / total_deposit as u128;
let r_a = (unclaimed as u128 * ratio_a / 1_000_000) as u64;
let r_b = unclaimed.saturating_sub(r_a);
```

Using 1M precision may cause precision loss at the 1 lamport level. When `deposit_a` / `total_deposit` is not evenly divisible by 1M, `r_a` will be rounded down and `r_b` will absorb the difference. This does not affect fund security (total is conserved), but the allocation ratio will have a minor deviation. It is recommended to use full `u128` precision division.

---

### DEV-16: `pipeline.rs:145` partial_transfer uses `-` instead of `checked_sub` (P3)

**File**: `src/pipeline.rs:145`

```rust
let updated_src = UTXOLeaf::standard(src_leaf.owner, src_leaf.amount - amount);
```

Since it has already been verified above that `src_leaf.amount >= amount`, this will not overflow. However, it is inconsistent with the style in `channel.rs`.

---

### DEV-17: `compliance.rs` `create_compliance_leaf` uses `hash_lock` field to store compliance_hash (P3)

**File**: `src/compliance.rs:287-296`

```rust
pub fn create_compliance_leaf(compliance_hash: [u8; 32]) -> UTXOLeaf {
    UTXOLeaf {
        leaf_type: LeafType::Compliance,
        owner: Pubkey::default(),
        amount: 0,
        hash_lock: Some(compliance_hash),  // reusing hash_lock field
        ...
    }
}
```

The `Compliance` type leaf reuses the `hash_lock` field to store `compliance_hash`. While this works functionally, it is semantically unclear. If HTLC-related logic is added in the future that traverses `hash_lock`, it may misidentify Compliance leaves.

---

### TEST-13: Missing `merge_spent_leaves` overflow test

**File**: `src/helpers.rs` tests

Missing boundary test for when `total_amount` accumulation may overflow (merging multiple large-amount leaves).

---

### TEST-14: Missing `merge_spent_leaves` target==source conflict test

**File**: `src/helpers.rs` tests

Missing error handling test for when `target_idx` is in `source_indices` (to verify DEV-12 scenario).

---

### TEST-15: Missing routing fee overflow test

**File**: `src/routing.rs` tests

Missing integer overflow test for large amounts combined with high `fee_rate_bps`.

---

### TEST-16: Missing negative test for `claim_leaf` rejecting HTLC leaves

**File**: `tests/channel_tests.rs`

The test `test_claim_leaf_and_htlc_verify_exclusive` actually demonstrates that `claim_leaf` **succeeds** on HTLC leaves (BUG-22), but there is no negative test verifying that `claim_leaf` should reject HTLC leaves.

---

### TEST-17: Missing `trigger_challenge` submitted_sequence == current_sequence boundary test

**File**: `src/channel.rs` tests

Missing boundary test for `submitted_sequence == current_sequence` (equal values). The code correctly rejects equal values by checking `submitted_sequence <= current_sequence`, but this boundary is not explicitly tested.

---

### TEST-18: Missing compliance slot=0 window behavior test

**File**: `src/compliance.rs` tests

All compliance tests use positive slot values. Since the channel manager passes `slot=0` to the compliance module, there should be tests verifying window pruning behavior when slot=0.

---

## II. On-Chain Code Audit (`ignite-pay-program/`)

### BUG-33: `submit_counter_state` does not verify any signatures (P0 — Fund Security)

**File**: `src/instructions/submit_counter_state.rs:22-23`

```rust
pub fn submit_counter_state(
    ...
    _sig_a: [u8; 64],  // underscore prefix indicates unused
    _sig_b: [u8; 64],  // underscore prefix indicates unused
) -> Result<()> {
```

`sig_a` and `sig_b` are marked as unused with underscore prefixes — **signatures are never verified**. Anyone can submit an arbitrary counter state, and only needs to provide a higher sequence number to override the channel state. This is a P0-level fund security vulnerability.

**Fix**: Use `verify_ed25519_signature` to verify `sig_a` against `user_pubkey` and `sig_b` against `provider_pubkey`. Message format: `channel_id || sequence || root`.

---

### BUG-34: `open_channel` does not verify `sig_a` (P0 — Fund Security)

**File**: `src/instructions/open_channel.rs`

The `open_channel` instruction accepts `initial_root` and `channel_id` as parameters but does not verify the user signature (`sig_a`). A malicious user could open a channel for any arbitrary pubkey. Design document §4.1 requires OpenChannel to have at least the user's signature.

**Fix**: Add a `sig_a: [u8; 64]` parameter and verify the signature of message `channel_id || deposit_a || tree_depth || initial_root` against `user_pubkey`.

---

### BUG-35: `claim.rs` / `verify_htlc.rs` / `htlc_refund.rs` do not execute SPL Token transfers (P1 — Fund Security)

**File**: `src/instructions/claim.rs`, `src/instructions/verify_htlc.rs`, `src/instructions/htlc_refund.rs`

These three instructions correctly verify Merkle proofs, signatures, etc., but **do not execute actual SPL Token transfers**. They only update the `total_claimed` record and `claimed_leaves` list. Users "claim" funds, but the tokens remain in escrow.

Unlike `finalize_settlement.rs` (which has CPI transfers implemented), the claim instructions are missing:
- `vault_a` / `vault_b` Token accounts
- `escrow_vault` Token account
- `token_program` Program
- `token::transfer` CPI call

**Fix**: Add SPL Token CPI transfer logic for claim/verify_htlc/htlc_refund.

---

### BUG-36: `open_channel` does not execute SPL Token deposit (P1 — Fund Security)

**File**: `src/instructions/open_channel.rs`

`open_channel` records `deposit_a` but **does not transfer tokens from the user account to escrow**. The channel state shows a deposit, but the actual tokens are not locked.

**Fix**: Add SPL Token CPI to transfer from `user_token_account` to `escrow_vault`.

---

### BUG-37: `cooperative_settle.rs:37` sequence check uses `==` instead of `>=` (P1)

**File**: `src/instructions/cooperative_settle.rs:37`

```rust
require!(
    sequence == channel.sequence,
    ChannelError::InvalidSequence
);
```

Design document §4.2 specifies that CooperativeSettle accepts `sequence >= on_chain.sequence`. The current implementation only accepts `==`. If the on-chain sequence is somehow lower than the latest off-chain sequence, a legitimate close request would be rejected.

For cooperative settle, `==` is actually reasonable (both parties always negotiate the latest state), but it needs to be confirmed whether this is consistent with the design document. If `>=` is allowed, an additional check is needed to verify that `root` matches.

---

### BUG-38: `finalize_settlement.rs` escrow_vault is missing PDA signing seeds (P1 — CPI Authority)

**File**: `src/instructions/finalize_settlement.rs:96-114`

```rust
let cpi_accounts_a = Transfer {
    from: ctx.accounts.escrow_vault.to_account_info(),
    to: ctx.accounts.vault_a.to_account_info(),
    authority: ctx.accounts.escrow_vault.to_account_info(),  // needs PDA signer
};
token::transfer(
    CpiContext::new(
        ctx.accounts.token_program.to_account_info(),
        cpi_accounts_a,
    ),
    refund_a,
)?;
```

`escrow_vault` serves as both `from` and `authority` in the `Transfer`, but `CpiContext::new` does not provide PDA signing seeds. The Solana Token program requires the authority to sign for transfers. Should use `CpiContext::new_with_signer` and provide PDA seeds.

---

### BUG-39: `verify_htlc.rs` is missing HTLC type validation (P2)

**File**: `src/instructions/verify_htlc.rs`

The instruction accepts `leaf_hash`, `hash_lock`, `preimage`, `timelock_slot`, and other parameters, but **does not verify that the leaf is actually of HTLC type**. An attacker could execute a VerifyHTLC operation on a Standard-type leaf (as long as they construct a matching hash_lock/preimage).

The off-chain `claim_htlc_verify` has this check at `channel.rs:1233`:
```rust
if leaf.leaf_type != LeafType::HTLC { return Err(...) }
```

The on-chain code is missing the corresponding validation.

---

### BUG-40: `verify_htlc.rs` / `htlc_refund.rs` do not verify that leaf parameters match actual leaf data (P2)

**File**: `src/instructions/verify_htlc.rs`, `src/instructions/htlc_refund.rs`

Callers pass in parameters such as `leaf_amount`, `hash_lock`, `timelock_slot`, `beneficiary`, etc., but on-chain only the Merkle proof (that `leaf_hash` is in the tree) and signatures are verified. **There is no verification that these parameters match the actual stored leaf data**.

For example, an attacker could:
1. Provide a correct Merkle proof (proving the leaf is in the tree)
2. But provide a tampered `leaf_amount` (higher) to claim more funds

The on-chain code is missing validation of the borsh-deserialized `leaf_data` (claim.rs has a partial implementation, but verify_htlc/htlc_refund do not).

---

### BUG-41: `settle_after_timeout.rs` does not check whether `settle_deadline` is None in Challenged state (P2)

**File**: `src/instructions/settle_after_timeout.rs`

When the channel is in `Challenged` state, `settle_deadline` may be `None` (because `trigger_challenge` does not set `settle_deadline`). However, both `verify_htlc.rs:49` and `htlc_refund.rs` require `settle_deadline` to not be None. This means HTLC operations in Challenged state may fail due to `settle_deadline = None`.

Design document §4.2 specifies that VerifyHTLC/HTLCRefund are also available in Challenged state, but on the condition that `settle_deadline` has been set.

---

### DEV-18: `open_channel.rs` tree_depth validation occurs after account initialization (P2)

**File**: `src/instructions/open_channel.rs`

The `tree_depth <= 8` validation is executed after `ChannelAccount::space(tree_depth)` is computed. If `tree_depth` is excessively large, `2usize.pow(tree_depth)` in `space()` will cause a panic (in debug mode) or produce an incorrect account size (in release mode).

The `tree_depth <= 8` validation should occur before the `space()` call.

---

### DEV-19: `fund_channel.rs` does not perform SPL Token deposit (P2)

**File**: `src/instructions/fund_channel.rs`

The off-chain `fund_channel` correctly creates provider leaves and updates `deposit_b`, but the on-chain `fund_channel` only updates `ChannelAccount`'s `deposit_b` and `leaf_count` — **it does not execute the SPL Token transfer from provider to escrow**.

---

### DEV-20: `verify_htlc.rs:52` settle_deadline check uses `<=` which is inconsistent with design document (P2)

**File**: `src/instructions/verify_htlc.rs:52`

```rust
require!(current_slot <= deadline, ...);
```

Design document §5 specifies that claim operations are available before `settle_deadline`; using `<=` means the operation is still allowed at the exact `deadline` time. The off-chain `channel.rs:978` uses `>` (rejecting only when `current_slot > deadline`), so the semantics are consistent. However, it needs to be confirmed that this works correctly with the `>=` (deadline) logic in `settle_after_timeout.rs`.

---

### PROG-12: On-chain `ed25519_dalek` should be replaced with Solana ed25519 syscall (P1)

**File**: `src/utils/ed25519.rs`

Currently using `ed25519_dalek::VerifyingKey::verify_strict()` for signature verification. On the Solana chain, it is recommended to use the `ed25519_program` syscall (via instruction introspection `InstructionError::InsufficientInstructions` or `solana_program::ed25519_instruction`) to verify Ed25519 signatures.

Reasons:
1. The Solana runtime has dedicated optimizations for the ed25519 syscall (parallel verification)
2. ed25519_dalek may have poor performance in BPF/CBV
3. The Anchor framework recommends using `ed25519_program` for signature verification

**Note**: This is an architectural optimization suggestion; `ed25519_dalek` is functionally correct. If the current implementation is retained for development simplicity, it should be documented.

---

### PROG-13: `claim.rs` does not verify the `amount` field in `leaf_data` (P2)

**File**: `src/instructions/claim.rs`

The BUG-20 fix added the `leaf_data` parameter and `InvalidLeafData` error, but the actual on-chain validation logic needs to deserialize `leaf_data` and verify that the `amount` field matches the `claim_amount` parameter. If the current implementation does not fully deserialize and validate, an attacker could tamper with the amount in `leaf_data`.

---

### PROG-14: `open_channel` is missing `payer == user_pubkey` constraint (P2)

**File**: `src/instructions/open_channel.rs`

In the OpenChannel account structure, the `payer`/`user` account is missing the `constraint = user.key() == channel.user_pubkey` constraint. Anyone can create a channel for any arbitrary user.

---

### PROG-15: `ChannelAccount` is missing on-chain handling for `auto_close_slot` (P3)

**File**: `src/instructions/settle_after_timeout.rs`

DEV-10 added the `auto_close_slot: Option<u64>` field in `state.rs`, but `settle_after_timeout` does not check this field. Design document §3.4.3 specifies: when `current_slot >= auto_close_slot`, anyone can trigger auto-settle (without requiring a challenge period).

---

### TEST-16: Missing on-chain `submit_counter_state` signature verification failure test

**File**: `ignite-pay-program/`

The current on-chain program does not have an integration test framework. The signature verification for `submit_counter_state` (after fixing BUG-28) needs corresponding tests to verify rejection of unsigned/incorrectly signed counter states.

---

## III. Issue Summary Table

### P0 Level (Fund Security — Must Fix)

| ID | Description | File |
|----|-------------|------|
| BUG-33 | `submit_counter_state` does not verify signatures | submit_counter_state.rs:22 |
| BUG-34 | `open_channel` does not verify user signature | open_channel.rs |
| BUG-35 | claim/verify_htlc/htlc_refund have no SPL Token transfers | claim.rs, verify_htlc.rs, htlc_refund.rs |

### P1 Level (Important Security Issues)

| ID | Description | File |
|----|-------------|------|
| BUG-22 | `claim_leaf` does not reject HTLC leaves | channel.rs:959 |
| BUG-23 | `trigger_challenge` signature uses current_slot | channel.rs:820 |
| BUG-25 | `merge_spent_leaves` overflow | helpers.rs:132,157 |
| BUG-36 | `open_channel` has no SPL Token deposit | open_channel.rs |
| BUG-37 | `cooperative_settle` sequence `==` vs `>=` | cooperative_settle.rs:37 |
| BUG-38 | `finalize_settlement` escrow missing PDA seeds | finalize_settlement.rs:96 |
| PROG-12 | On-chain should use ed25519 syscall | utils/ed25519.rs |

### P2 Level (Functional Correctness)

| ID | Description | File |
|----|-------------|------|
| BUG-24 | partial_transfer operation order reversed vs design document | pipeline.rs:130, helpers.rs:63 |
| BUG-26 | split_from_rest `-` vs checked_sub | helpers.rs:81 |
| BUG-27 | routing fee calculation overflow | routing.rs:187 |
| BUG-28 | Liquidity check does not include fees | routing.rs:194 |
| BUG-29 | compliance slot=0 window calculation error | channel.rs:409 |
| BUG-30 | compliance threshold semantic inconsistency | compliance.rs:188 |
| BUG-31 | htlc timelock calculation overflow | htlc.rs:133 |
| BUG-32 | Challenged state settle_deadline not set | channel.rs:1190 |
| BUG-39 | verify_htlc missing HTLC type validation | verify_htlc.rs |
| BUG-40 | verify_htlc/htlc_refund parameters not verified | verify_htlc.rs, htlc_refund.rs |
| BUG-41 | Challenged state settle_deadline may be None | settle_after_timeout.rs |
| DEV-12 | merge target==source overwrite | helpers.rs:157 |
| DEV-14 | score_route fee overflow | routing.rs:238 |
| DEV-15 | Signature message format inconsistent with design document | signing.rs:28 |
| DEV-18 | tree_depth validation order | open_channel.rs |
| DEV-19 | fund_channel has no SPL Token deposit | fund_channel.rs |
| DEV-20 | settle_deadline <= vs < | verify_htlc.rs:52 |
| PROG-13 | claim leaf_data amount not verified | claim.rs |
| PROG-14 | open_channel missing user constraint | open_channel.rs |

### P3 Level (Code Quality / Suggestions)

| ID | Description | File |
|----|-------------|------|
| DEV-13 | select_best_route assumes sorted input | routing.rs:285 |
| DEV-16 | Refund precision loss | channel.rs:1441 |
| DEV-17 | partial_transfer `-` vs checked_sub | pipeline.rs:145 |
| DEV-21 | compliance leaf reuses hash_lock | compliance.rs:287 |
| PROG-15 | auto_close_slot not handled on-chain | settle_after_timeout.rs |

### TEST Gaps

| ID | Description | Priority |
|----|-------------|----------|
| TEST-13 | merge_spent_leaves overflow boundary test | P2 |
| TEST-14 | merge target==source conflict test | P2 |
| TEST-15 | routing fee overflow test | P2 |
| TEST-16 | claim_leaf rejects HTLC leaves test | P1 |
| TEST-17 | trigger_challenge sequence==current test | P2 |
| TEST-18 | compliance slot=0 window behavior test | P2 |

---

## IV. Fix Priority Recommendations

### First Priority: P0 Security Vulnerabilities (3 issues)

1. **BUG-33**: `submit_counter_state` add dual-signature verification
2. **BUG-34**: `open_channel` add user signature verification
3. **BUG-35**: claim/verify_htlc/htlc_refund add SPL Token CPI transfers

### Second Priority: P1 Issues (7 issues)

4. **BUG-22**: `claim_leaf` add `leaf_type != Standard` rejection check
5. **BUG-23**: `trigger_challenge` signature switch to `submitted_sequence`
6. **BUG-25**: `merge_spent_leaves` use `saturating_add`
7. **BUG-36**: `open_channel` add SPL Token deposit CPI
8. **BUG-38**: `finalize_settlement` use `CpiContext::new_with_signer` + PDA seeds
9. **BUG-37**: Confirm cooperative_settle sequence semantics
10. **PROG-12**: Evaluate ed25519_dalek vs syscall

### Third Priority: P2 Functional Issues (19 issues)

Fix grouped by functional module:
- **Routing module**: BUG-27, BUG-28, DEV-14
- **Compliance module**: BUG-29, BUG-30
- **On-chain claim**: BUG-39, BUG-40, BUG-41, PROG-13, PROG-14
- **On-chain other**: DEV-18, DEV-19, DEV-20
- **helpers**: BUG-24, DEV-12
- **Signing**: DEV-15
- **HTLC**: BUG-31, BUG-32

---

## V. Design Document Alignment Check

| Design Document Section | Off-Chain Implementation | On-Chain Implementation | Difference Notes |
|------------------------|------------------------|------------------------|------------------|
| §3.1 OpenChannel | ✅ Complete | ⚠️ Missing signature + deposit | BUG-29, BUG-31 |
| §3.2 SplitTree | ✅ Complete | N/A | |
| §3.3 LeafUpdate | ✅ Complete | N/A | |
| §3.4.1 CooperativeClose | ✅ Complete | ⚠️ sequence `==` | BUG-32 |
| §3.4.2 Challenge/Settle | ✅ Complete | ⚠️ settle_deadline | BUG-36 |
| §3.4.3 AutoClose | ✅ Complete | ⚠️ Not handled | PROG-15 |
| §4.2 Claim | ✅ Complete | ⚠️ Missing SPL transfer | BUG-30 |
| §4.2 VerifyHTLC | ✅ Complete | ⚠️ Missing type check + transfer | BUG-34, BUG-35 |
| §4.2 HTLCRefund | ✅ Complete | ⚠️ Missing transfer | BUG-30 |
| §4.2 SubmitCounter | ✅ Complete | ❌ Missing signature verification | BUG-28 |
| §5.4 Finalize | ✅ Complete | ⚠️ Missing PDA seeds | BUG-33 |
| §6 Signing | ✅ Complete | ✅ Complete | |
| §10.3 Routing | ⚠️ Overflow | N/A | BUG-24, BUG-25 |
| §10.7 Compliance | ⚠️ slot issue | N/A | BUG-26, BUG-27 |

---

## VI. Conclusion

The off-chain code (`ignite-pay-state-channel`) has good overall quality. The core payment flow (Open → Split → Transfer → HTLC → Close → Claim → Finalize) is fully implemented. The main issues are concentrated in:
1. Insufficiently comprehensive integer overflow protection (helpers, routing)
2. Slot passing issue in the compliance module

The on-chain code (`ignite-pay-program`) has multiple critical security vulnerabilities:
1. `submit_counter_state` has no signature verification at all (anyone can submit forged states)
2. Claim instructions are missing SPL Token transfers (funds are not actually transferred)
3. `open_channel`/`fund_channel` are missing Token deposits
4. `finalize_settlement` CPI is missing PDA signing seeds

**Recommendation**: Prioritize fixing the 3 P0-level issues (BUG-28/29/30), then systematically fix P1/P2 issues in priority order. The on-chain program must complete all P0 and P1 fixes before deployment.
