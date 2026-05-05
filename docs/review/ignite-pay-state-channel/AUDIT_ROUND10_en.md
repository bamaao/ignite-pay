# Ignite Pay State Channel Code Audit Report (Round 10)

> Audit basis: `docs/utxo_merkletree_state_channel.md` (design specification v2.0)
> Audit scope: All source code in `ignite-pay-state-channel/` + on-chain programs in `ignite-pay-program/`
> Audit date: 2026-04-11 (Round 10)

---

## Audit Overview

Each item was checked against the design document for business process implementation completeness, rule consistency, test coverage, and potential bugs. A total of **8 BUGs**, **6 business rule deviations**, and **7 test coverage gaps** were found.

---

## 1. BUG List

### BUG-10: `fund_channel` signature is created but never verified

**Severity**: High
**File**: `src/channel.rs:204`
**Design document**: §2.B LeafUpdate signature = `SHA-256(channel_id || sequence || leaf_index || prev_leaf_hash || new_leaf_hash)`

```rust
let _update = crate::signing::sign_leaf_update(
    &state.metadata.channel_id,
    new_sequence,
    target_index as u32,
    &prev_leaf,
    new_leaf.clone(),
    provider_keypair,
);
// _update is created but discarded; the signature is never verified for correctness
```

**Issue**: The signature is created and immediately discarded (`_update`), with no persistence or return to the caller. This results in:
1. Broken audit trail: This LeafUpdate is not recorded to sled, making it impossible to trace later
2. If the signature computation is incorrect, it will not be detected

**Recommendation**: Change `_update` to `update`, and persist it to sled or at minimum return it to the caller.

---

### BUG-11: Regression bug in `construct_split_tree` for single-party funded channels

**Severity**: Medium
**File**: `src/channel.rs:271-282`
**Design document**: §3.1.3 "all leaf owners = user"

After FLOW-3 modifications, when `deposit_b == 0` (single-party funding), `provider_total` must equal 0. If no provider leaves exist in the leaves, `provider_total` being 0 is correct. However, the original logic required "all leaf owners must be user", while the new logic simply does not error -- this is not a bug in itself, but **if there are empty leaves** (amount=0, owner=Pubkey::default()), the empty leaf's owner is neither user nor provider, and would be rejected.

```rust
for (i, leaf) in leaves.iter().enumerate() {
    if !leaf.is_empty() {
        if leaf.owner == user_pubkey { ... }
        else if leaf.owner == provider_pubkey { ... }
        else { return Err(...) }  // Empty leaf owner=Pubkey::default() does not match
    }
}
```

**Analysis**: `if !leaf.is_empty()` checks `amount == 0`, so empty leaves are skipped. **This will not actually trigger.** However, the semantics are not clear enough -- it is recommended to add comments explaining why empty leaves are skipped.

**Conclusion**: Non-blocking issue; the code logic is correct but readability could be improved.

---

### BUG-12: `MIN_TIMELOCK_BASE` hardcoded to `500` instead of using `challenge_duration`

**Severity**: Medium
**File**: `src/channel.rs:17`
**Design document**: §10.4.2 `MIN_TIMELOCK = CHALLENGE_DURATION + 3 * HOP_MARGIN`

```rust
pub const MIN_TIMELOCK_BASE: u64 = 500 + 3 * HOP_MARGIN;
```

The design document states `MIN_TIMELOCK = CHALLENGE_DURATION + 3 * HOP_MARGIN`, where `CHALLENGE_DURATION` is a channel parameter. But the code hardcodes `500` and does not vary with the channel's `challenge_duration`.

**Impact**: If the channel's `challenge_duration` is greater than 500, the timelock for multi-hop HTLCs may not be long enough, preventing the service provider from submitting the preimage on-chain in time.

**Recommendation**: Change `MIN_TIMELOCK_BASE` to a function that accepts a `challenge_duration` parameter:
```rust
pub fn min_timelock(challenge_duration: u64) -> u64 {
    challenge_duration + 3 * HOP_MARGIN
}
```

---

### BUG-13: `split_from_rest` operation order is inconsistent with the design document

**Severity**: Low
**File**: `src/helpers.rs:56-88`
**Design document**: §3.2.2 "Must **deduct from Rest first**, then create a new UTXO in the free slot"

The code comment states "BUG-6 fix: Creates target FIRST", meaning the **target leaf is created first, then Rest is deducted**. But design document §3.2.2 explicitly requires deducting from Rest first:

> "A split is essentially two atomic LeafUpdates -- you must first deduct from Rest, then create a new UTXO in the free slot. This order ensures the conservation invariant is not broken in any intermediate state"

The code comment explains that creating the target first (total amount increases) then deducting from Rest (total amount restores) guarantees `sum(leaves) >= total_deposited`. But the design document's conservation invariant is `sum(leaves) == total_deposited` (exact equality).

**Analysis**: The code implements a relaxed conservation of "intermediate state total >= deposit total" rather than the "exact conservation at each step" required by the design document. Both approaches are safe (no funds are created out of thin air), but this is inconsistent with the design document's description.

**Recommendation**: Add a code comment explicitly explaining the deviation from the design document and its rationale.

---

### BUG-14: `claim_leaf` lacks a Merkle proof parameter

**Severity**: Medium
**File**: `src/channel.rs:831`
**Design document**: §5.2 Claim process "submit (leaf_index, leaf_data, merkle_proof)"

The design document requires the caller to provide a Merkle proof during claim (because the on-chain contract needs it). But in the off-chain implementation, `claim_leaf` generates the proof directly from `state.tree` internally (`state.tree.get_proof(leaf_index)`), without accepting an external proof parameter.

```rust
// BUG: Code generates proof itself instead of verifying a proof submitted by the caller
let proof = state.tree.get_proof(leaf_index as usize)?;
```

**Impact**: The off-chain API is inconsistent with the on-chain Claim instruction interface. On-chain requires the caller to provide a proof; off-chain auto-generates one -- this may cause confusion during integration.

**Recommendation**: Add a `claim_leaf_with_proof` variant that accepts an external proof parameter, aligning with the on-chain interface.

---

### BUG-15: `claim_htlc_verify` and `claim_htlc_refund` are only available in Settling state

**Severity**: Low
**File**: `src/channel.rs:952, 1040`
**Design document**: §4.2 VerifyHTLC/HTLCRefund "**Challenged or Settling status**"

The design document explicitly states that VerifyHTLC and HTLCRefund can be used in both **Challenged and Settling** states. But the code only checks for `ChannelStatus::Settling`:

```rust
if state.metadata.status != ChannelStatus::Settling {
    return Err(...);
}
```

**Impact**: During the challenge period, the service provider cannot submit a preimage via VerifyHTLC to prove it has completed service.

**Recommendation**: Change to `status == Challenged || status == Settling`.

---

### BUG-16: `close_channel` checks `signed_state.sequence == state.metadata.sequence` instead of `>=`

**Severity**: Low
**File**: `src/channel.rs:599`
**Design document**: §4.2 CooperativeSettle "sequence > on_chain.sequence"

The design document's CooperativeSettle requires `sequence > on_chain.sequence`, but the code checks for strict equality:

```rust
if signed_state.sequence != state.metadata.sequence {
    return Err(...);
}
```

**Analysis**: In the off-chain implementation, `state` already holds the latest sequence from both parties, so the strict equality check is reasonable (complementary to the `>` check in `submit_counter_state`). However, the on-chain program `cooperative_settle.rs` also checks `==`. This differs from the design document's description.

**Recommendation**: Add a code comment explaining why `==` is used instead of `>`.

---

### BUG-17: `routing.rs` DFS route discovery may produce duplicate routes

**Severity**: Low
**File**: `src/routing.rs:107-138`

`dfs_routes` uses `visited: Vec<[u8; 32]>` for visited checks, using linear search via `visited.iter().any(|v| v == neighbor)`. Linear search on `[u8; 32]` types is inefficient and may cause performance issues in large-scale networks.

Additionally, the path space of `discover_routes` grows exponentially with the number of hubs, with no pruning strategy, which may cause search timeouts in large-scale networks.

---

## 2. Business Rule Deviations

### DEV-1: `auto_close_slot` trigger logic is not implemented

**Design document**: §3.4.3 "When current_slot >= auto_close_slot, anyone can trigger settlement"
**File**: `src/channel.rs`

`ChannelMetadata` has an `auto_close_slot: Option<u64>` field, which `open_channel` initializes to `None`, but there is no method to set or trigger auto-close. Design document §3.4.3 describes the Auto-close path, but the code is missing:
- `set_auto_close_slot()` method
- `auto_settle()` method (checks `current_slot >= auto_close_slot` then directly enters Settling)

---

### DEV-2: Compliance module is not integrated with channel operations

**Design document**: §6 "Add constraints in off-chain business logic; when cumulative payment amount triggers a threshold, automatically insert a compliance marker"
**Files**: `src/compliance.rs`, `src/channel.rs`

`ComplianceManager` exists independently, but `apply_leaf_update` and `transfer_leaf` (pipeline.rs) do not call `compliance.record_payment()`. Compliance checks are standalone and not embedded in the payment flow.

---

### DEV-3: Route scoring formula is inconsistent with the design document

**Design document**: §10.3.2
```rust
let fee_score = 1.0 / (1.0 + total_fee as f64 / amount as f64);
let latency_score = 1.0 / (1.0 + max_latency as f64 / 1000.0);
```

**Code**: `src/routing.rs:244-268`
```rust
let fee_score = (1.0 - fee_ratio).max(0.0);  // fee_ratio = total_fee / amount
let latency_score = (1.0 - (max_latency_ms as f64 / 10000.0)).max(0.0);
```

The two formulas produce different results. The design document uses the inverse `1/(1+x)`, while the code uses linear truncation `1-x`. When the fee rate is high, the code's score may go negative and then be truncated to 0, whereas the design document's formula always remains positive.

---

### DEV-4: Design document §4.2 `TriggerChallenge` requires `sequence > on_chain.sequence`

**File**: `src/channel.rs:665-710`

Design document §4.2 table states TriggerChallenge requires "submit (root, sequence, sig), verify sequence > on_chain.sequence". But the code's `trigger_challenge` does not accept sequence/root parameters; it only verifies the signer and signature. The code does not check whether the submitted sequence is greater than the on-chain sequence.

**Analysis**: In the off-chain version, channel state is already maintained by both parties, so the sequence check is only meaningful in the on-chain contract. However, to align with on-chain logic, a sequence parameter should be added.

---

### DEV-5: Design document §10.4.1 route fees implemented via HTLC amount difference

**File**: `src/multihop.rs`

Design document §10.5 describes route fees as being implicitly implemented through decreasing HTLC amounts at each hop. But in `MultiHopEntry`, each hop's `amount` is specified independently, and `create_payment`'s `hops_metadata` allows the caller to freely set each hop's amount -- there is no automatic fee decrement calculation.

**Recommendation**: Add a helper function to automatically calculate per-hop amounts based on hub fee rates.

---

### DEV-6: `settle_after_timeout` uses `>=` instead of the design document's `>`

**Design document**: §4.2 SettleAfterTimeout "`current_slot > challenge_slot + challenge_duration`"
**File**: `src/channel.rs:734`

```rust
if current_slot < challenge_slot + state.metadata.challenge_duration {
    return Err(...);
}
```

The code allows settlement when `current_slot == challenge_slot + challenge_duration` (`>=`), while the design document requires strict greater-than (`>`). This difference also exists in `htlc::check_expiry` where `current_slot > record.timelock_slot` (strict greater-than) is used -- but settle uses `>=`, which is inconsistent.

---

## 3. Test Coverage Gaps

### TEST-1: Missing integration test for `construct_split_tree` after `fund_channel`

**File**: None

Design document §10.6.2 describes the dual-funding flow: first `fund_channel`, then `construct_split_tree`. Current tests cover both methods individually, but there is no test for the complete **fund-then-split** end-to-end flow.

---

### TEST-2: Missing integration test for compliance module with channel

**File**: None

No test exists for the complete flow: "channel payment triggers compliance threshold -> insert Compliance leaf -> channel paused -> hold cleared -> resumed".

---

### TEST-3: Missing multi-hop payment end-to-end test

**File**: None

Tests in `multihop.rs` cover individual `MultiHopManager` operations, but there is no test for the cross-channel HTLC chained lock/unlock flow.

---

### TEST-4: Missing tests for VerifyHTLC/HTLCRefund in Challenged state

**File**: `src/channel.rs` tests

Design document §4.2 and §3.4.2 Scenario B describe the service provider submitting VerifyHTLC in the Challenged state, but current tests only cover the Settling state. Furthermore, due to BUG-15, these operations would be rejected in the Challenged state.

---

### TEST-5: Missing `auto_close_slot` related tests

**File**: None

The auto-close flow described in design document §3.4.3 has no test coverage at all.

---

### TEST-6: Missing edge case tests for duplicate claims

`claim_leaf` has a `claimed_leaves` set to prevent duplicate claims, but there are no tests verifying:
- The same leaf cannot be claimed by `claim_htlc_verify` and then `claim_leaf`
- `claim_leaf` and `claim_htlc_refund` are mutually exclusive

---

### TEST-7: Missing edge case tests for multi-hop decreasing timelocks

`multihop.rs` tests 3-hop and 1-hop timelocks, but is missing:
- Tests that timelocks for maximum hop counts (e.g., 10+ hops) do not overflow or become negative
- Edge case for `HOP_MARGIN = 0`

---

## 4. On-Chain Program (ignite-pay-program) Audit

### PROG-1: `Claim` instruction does not check the `claimed_leaves` set

**File**: `ignite-pay-program/src/instructions/claim.rs`
**Design document**: §5.2 "duplicate claim prevention"

The design document requires the on-chain contract to maintain `claimed_leaves: Set<u32>` to prevent duplicate claims. But in the Anchor program, `ChannelAccount` does not have a `claimed_leaves` field (design document §4.1 includes this field), and the Claim instruction does not check whether the leaf has already been claimed.

**Cause**: `ChannelAccount` state is missing the `claimed_leaves: Vec<u32>` field.

---

### PROG-2: `FinalizeSettlement` does not actually execute SPL Token transfers

**File**: `ignite-pay-program/src/instructions/finalize_settlement.rs:77`

```rust
let _ = (refund_a, refund_b); // Used in production CPI calls
```

`refund_a` and `refund_b` are computed and then discarded, with no actual CPI transfer executed. Marked as TODO status.

---

### PROG-3: `ChannelAccount` is missing fields required by the design document

**Design document**: §4.1 ChannelAccount includes `claimed_leaves: Vec<u32>`, `leaf_count: u32`
**File**: `ignite-pay-program/src/state.rs`

The on-chain `ChannelAccount` is missing the `claimed_leaves` field, which is a critical data structure for preventing duplicate claims.

---

### PROG-4: `TriggerChallenge` does not verify `sequence > on_chain.sequence`

**File**: `ignite-pay-program/src/instructions/trigger_challenge.rs`
**Design document**: §4.2 TriggerChallenge "verify sequence > on_chain.sequence"

The on-chain `trigger_challenge` instruction has no sequence/root parameters and cannot verify that the submitted sequence is greater than the on-chain recorded sequence.

---

### PROG-5: Ed25519 signature verification is a placeholder

**File**: All instruction files

Comments state "Ed25519 signature verification is done via Solana instruction introspection", but actual Ed25519 instruction verification is not implemented. Needs to be used with `solana_sdk::ed25519_instruction`.

---

## 5. Summary

| Category | Count | Severity distribution |
|----------|-------|----------------------|
| BUG | 8 | High 1, Medium 3, Low 4 |
| Business rule deviations | 6 | - |
| Test coverage gaps | 7 | - |
| On-chain program issues | 5 | - |
| **Total** | **26** | - |

### Priority Recommendations

**P0 (Must fix)**:
- BUG-10: fund_channel signature not persisted
- BUG-12: MIN_TIMELOCK_BASE hardcoded
- BUG-15: VerifyHTLC/HTLCRefund should support Challenged state
- PROG-1/3: On-chain claimed_leaves missing

**P1 (Should fix)**:
- DEV-1: auto_close_slot trigger logic
- DEV-6: settle_after_timeout `>` vs `>=` inconsistency
- BUG-14: claim_leaf missing external proof parameter
- TEST-4: VerifyHTLC test in Challenged state

**P2 (Suggested improvements)**:
- DEV-2: Compliance module integration
- DEV-3: Scoring formula alignment
- DEV-4/5: Interface detail alignment
- BUG-13/16: Comments explaining design deviations
- Remaining test coverage gaps
