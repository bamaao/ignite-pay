# Ignite Pay State Channel Round 12 Code Audit Report

> Audit basis: `docs/utxo_merkletree_state_channel.md` (design specification)
> Audit scope: `ignite-pay-state-channel/` project + `ignite-pay-program/` on-chain program
> Audit date: 2026-04-12 (Round 12)

---

## Audit Overview

All source files were reviewed against the design document, checking business logic, functional rules, test coverage, and potential bugs. This report classifies findings by severity:

- **BUG**: Functional defects that may cause fund safety issues
- **DEV**: Business rule deviations from the design document
- **TEST**: Missing test coverage
- **PROG**: On-chain program issues

| Category | Findings |
|----------|----------|
| BUG | 4 |
| DEV | 5 |
| TEST | 5 |
| PROG | 6 |
| **Total** | **20** |

---

## 1. BUG (Functional Defects)

### BUG-18: On-chain `verify_htlc` missing timelock check

**Severity**: P1 (Fund Safety)
**File**: `ignite-pay-program/src/instructions/verify_htlc.rs`
**Design Document**: §4.2 VerifyHTLC — "timelock not expired (current_slot <= timelock_slot)"
**Issue**: The on-chain `verify_htlc` function accepts `preimage` and `hash_lock` parameters and verifies the hash_lock match, but **does not check at all whether the timelock has expired**. An attacker could use the preimage to claim funds even after the HTLC has expired (when the owner should be able to refund), creating a race condition with HTLCRefund.
**Recommendation**: Add `require!(current_slot <= timelock_slot, ChannelError::HtlcExpired)` or pass `timelock_slot` as a function parameter and verify it.

### BUG-19: On-chain `settle_after_timeout` uses `>=` instead of strict `>`

**Severity**: P2
**File**: `ignite-pay-program/src/instructions/settle_after_timeout.rs:30`
**Design Document**: §4.2 — "current_slot > challenge_slot + challenge_duration" (strict greater than)
**Issue**: The on-chain code uses `current_slot >= challenge_slot + channel.challenge_duration` (greater than or equal), while the off-chain code in `channel.rs:800` correctly uses strict `>` (rejecting when `current_slot <= challenge_slot + ...`). The on-chain and off-chain behaviors are inconsistent; the on-chain version allows entering the Settling state one slot earlier.
**Recommendation**: Change to `require!(current_slot > challenge_slot + channel.challenge_duration, ...)`.

### BUG-20: On-chain `claim` instruction does not verify consistency between `claim_amount` and the provided `leaf_hash`

**Severity**: P2
**File**: `ignite-pay-program/src/instructions/claim.rs`
**Issue**: The `claim` instruction accepts `claim_amount` and `leaf_hash` parameters, verifies the Merkle proof and claimer==leaf_owner, but **has no mechanism to ensure `claim_amount` is actually encoded in `leaf_hash`**. A caller could pass `claim_amount=1` with a valid `leaf_hash` (corresponding to a leaf with amount=1000000); the Merkle proof would pass but only 1 unit would be claimed. The off-chain code has a `claim_amount != leaf.amount` check at `channel.rs:977`, but the on-chain code cannot perform this check (because it does not deserialize the leaf).
**Note**: This is determined by the Merkle tree design — on-chain only verifies hashes, not content. This needs to be prevented through other mechanisms (such as requiring the serialized leaf data to be passed in, or encoding the amount in the `leaf_hash` off-chain). The current off-chain `claim_leaf` has this check at `channel.rs:977`, but it is missing on-chain.
**Recommendation**: Consider requiring the caller to pass in the full serialized leaf data, recompute the hash on-chain and compare, while also verifying the claim_amount.

### BUG-21: `routing.rs` channel graph construction assumes fully-connected topology

**Severity**: P3
**File**: `ignite-pay-state-channel/src/routing.rs:81-97`
**Design Document**: §10.3.1 Channel graph should be constructed based on actual on-chain channel data
**Issue**: `refresh_graph()` treats all registered Hubs as fully interconnected (complete graph), ignoring actual channel states and liquidity topology. Although the comments note this is a simplified model, it may discover non-existent routes (between Hub pairs with no actual channel), which would cause payment failures in a production environment.
**Recommendation**: In a production implementation, the channel graph should be constructed from on-chain ChannelAccount data, including only actually existing bidirectional channels.

---

## 2. DEV (Business Rule Deviations)

### DEV-7: Route scoring uses average `success_rate` instead of design document's `min_success_rate`

**File**: `ignite-pay-state-channel/src/routing.rs:253`
**Design Document**: §10.3.2 — reliability_score should use `min(success_rate)` to take the lowest success rate of the entire path
**Issue**: The code uses `avg_success = sum(success_rate) / count` (average success rate), while the design document explicitly specifies using `min`. A route containing a low-reliability Hub would score significantly lower under `min` mode than `avg` mode; the current implementation may select routes that pass through unreliable Hubs.
**Recommendation**: Change `avg_success` to `min_success`: `let min_success = metrics.iter().map(|m| m.success_rate as f64 / 10000.0).fold(f64::INFINITY, f64::min);`

### DEV-8: Compliance module not integrated with channel operations

**File**: `ignite-pay-state-channel/src/channel.rs` — `apply_leaf_update` method
**Design Document**: §11.2.3 — When cumulative_spent >= threshold, compliance_hold should be triggered
**Issue**: `apply_leaf_update` does not call `ComplianceManager::record_payment()` when processing leaf updates. The compliance module is entirely standalone and does not automatically trigger during actual payment flows.
**Status**: Flagged as P2 in previous audit rounds (DEV-2), still unfixed.

### DEV-9: `trigger_challenge` off-chain version missing sequence/root parameters

**File**: `ignite-pay-state-channel/src/channel.rs:730`
**Design Document**: §4.2 TriggerChallenge — should submit (submitted_root, submitted_sequence)
**Issue**: The off-chain `trigger_challenge` only accepts challenger_pubkey and signature, not submitted_root and submitted_sequence. The on-chain version `trigger_challenge.rs` correctly accepts these two parameters and updates on-chain state. The off-chain version does not update current_root and sequence after triggering a challenge.
**Status**: Flagged as P2 in previous audit rounds (DEV-4), still unfixed.

### DEV-10: On-chain `ChannelAccount` missing `auto_close_slot` field

**File**: `ignite-pay-program/src/state.rs`
**Design Document**: §4.1 ChannelAccount layout table includes `auto_close_slot: Option<u64>` (§3.4.3 auto-close feature)
**Issue**: The off-chain `ChannelMetadata` (types.rs:218) has an `auto_close_slot: Option<u64>` field, but the on-chain `ChannelAccount` (state.rs:21-64) **is missing this field**. There is no corresponding on-chain auto-close instruction; the auto_settle feature is only implemented off-chain.
**Recommendation**: Add `pub auto_close_slot: Option<u64>` field to `ChannelAccount` and add a corresponding on-chain auto_settle instruction.

### DEV-11: `window_slots` sliding window not implemented

**File**: `ignite-pay-state-channel/src/compliance.rs`
**Design Document**: §11.2.1 SpendingLimit includes a `window_slots` field for sliding window rate limiting
**Issue**: The `SpendingLimit` struct has a `window_slots` field, but `record_payment()` only checks `cumulative_spent >= threshold` without using a sliding window. `cumulative_spent` is a cumulative value that never decays. This means once cumulative spending exceeds the threshold, it is permanently frozen rather than resetting after the window period.
**Recommendation**: Implement sliding window logic based on `window_slots`, or remove the `window_slots` field and update the design document.

---

## 3. TEST (Missing Test Coverage)

### TEST-8: Missing on-chain program unit/integration tests

**File**: `ignite-pay-program/`
**Issue**: The on-chain program (10 Anchor instructions) has no test files at all. While Anchor programs are typically verified through integration tests, core logic (such as Merkle proof verification, amount overflow checks, state transition constraints) should have independent unit tests.
**Recommendation**: Add tests for at least the following critical logic:
- Merkle proof verification (`utils/merkle.rs`)
- Duplicate claim rejection in the Claim instruction
- hash_lock verification in VerifyHTLC
- Expiration check in HTLCRefund
- Proportional refund calculation in FinalizeSettlement

### TEST-9: Missing compliance module and channel integration tests

**Issue**: No tests verify whether the compliance module is correctly called during `apply_leaf_update`. The compliance module is currently completely independent of channel operations, but the design document requires the two to be integrated.
**Status**: Flagged as P2 in previous audit rounds (TEST-2), still unfixed.

### TEST-10: Missing multi-hop payment end-to-end tests

**Issue**: No tests verify the cross-channel HTLC chain lock/unlock flow. Tests in `multihop.rs` only verify single-module creation/parsing logic and do not cover HTLC coordination across multiple channels.
**Status**: Flagged as P2 in previous audit rounds (TEST-3), still unfixed.

### TEST-11: Missing `claim_leaf` rejection test for leaves owned by non-channel participants

**File**: `ignite-pay-state-channel/tests/channel_tests.rs`
**Issue**: In the `test_full_lifecycle_with_htlc_and_settlement` test (line 506 comment), the merchant-owned leaf cannot be claimed by the user or provider (because claim_leaf checks claimer==leaf.owner), but this important boundary condition lacks a dedicated test case.
**Recommendation**: Add tests verifying: when the leaf owner is a non-channel participant, claim_leaf should be rejected.

### TEST-12: Missing `compute_hop_amounts` large-value overflow test

**File**: `ignite-pay-state-channel/src/multihop.rs`
**Issue**: `compute_hop_amounts` uses `checked_mul`/`checked_add` to handle overflow, but there are no tests verifying the `None` return behavior on overflow.
**Recommendation**: Add test cases: when destination_amount is close to u64::MAX with multi-hop fees, it should return `None`.

---

## 4. PROG (On-chain Program Issues)

### PROG-6: `FundChannel` on-chain instruction does not update `leaf_count`

**File**: `ignite-pay-program/src/instructions/fund_channel.rs:47-52`
**Issue**: The on-chain `open_channel` sets `leaf_count = 1`, but `fund_channel` does not update `leaf_count` after updating `deposit_b` and `total_deposited`. The off-chain code updates `leaf_count` during `fund_channel` (channel.rs:226); the on-chain behavior is inconsistent.
**Recommendation**: Add `channel.leaf_count += 1;` to the on-chain `fund_channel`.

### PROG-7: `FinalizeSettlement` does not execute actual SPL Token CPI transfer

**File**: `ignite-pay-program/src/instructions/finalize_settlement.rs:84-89`
**Design Document**: §11.4.4 — "FinalizeSettlement: SPL Token transfer CPI from escrow -> vault_a / vault_b"
**Issue**: Although the code correctly calculates `refund_a` and `refund_b`, the actual transfer logic is commented out (`let _ = (refund_a, refund_b);`). Users will not receive refunds.
**Status**: Flagged as P2 in previous audit rounds (PROG-2), still unfixed.

### PROG-8: Ed25519 signature verification uses placeholders

**File**: Multiple on-chain instructions (`cooperative_settle.rs`, `trigger_challenge.rs`, `submit_counter_state.rs`, `claim.rs`, `verify_htlc.rs`, `htlc_refund.rs`, `finalize_settlement.rs`)
**Design Document**: §11.4.4 — "Ed25519 signature verification via Solana ed25519_program instruction introspection"
**Issue**: Signature parameters in all instructions are named with underscore prefixes (`_sig_a`, `_sig_b`, `_claimer_signature`, `_caller_signature`, `_challenger_signature`), indicating these parameters are received but unused. Signature verification relies on an "external mechanism" (Ed25519 instruction introspection), but the code has no actual integration of this mechanism.
**Status**: Flagged as P2 in previous audit rounds (PROG-5), still unfixed.

### PROG-9: `ChannelAccount::space()` calculation is imprecise

**File**: `ignite-pay-program/src/state.rs:68-91`
**Issue**: The `status` field uses a `1 + 32` estimate (1 byte enum + 32 padding), but the actual Anchor-serialized `ChannelStatus` enum only occupies 1 byte and should not have 32 bytes of padding. `claimed_leaves` uses `4 + 256 * 4` (up to 256 entries), but `open_channel` has no initialization upper bound check. If the number of leaves exceeds 256 (when tree_depth > 8), the claim operation may cause account space overflow.
**Recommendation**: Fix the space calculation, or limit `tree_depth <= 8` in `open_channel`.

### PROG-10: `trigger_challenge` directly accepts and stores submitted root/sequence without verifying signatures

**File**: `ignite-pay-program/src/instructions/trigger_challenge.rs:53-54`
**Issue**: `trigger_challenge` accepts `submitted_root` and `submitted_sequence` and writes them directly to on-chain state, only noting via comments that signature verification is done via "Ed25519 instruction introspection". If the signature verification mechanism is not properly integrated, anyone could submit arbitrary root/sequence values to overwrite channel state.
**Status**: Related to PROG-8.

### PROG-11: `cooperative_settle` uses `==` to compare sequence, while off-chain close allows current sequence

**File**: `ignite-pay-program/src/instructions/cooperative_settle.rs:37-44`
**Issue**: The on-chain `cooperative_settle` requires `sequence == channel.sequence`, which is correct on-chain (since on-chain sequence represents the finally confirmed state). However, the off-chain `close_channel` sequence check behavior (channel.rs:611) uses strict equality with a comment noting the deviation. The behaviors are consistent in both places, but the on-chain `cooperative_settle` does not perform substantive verification of the submitted root and signatures (only placeholders); it needs to ensure Ed25519 verification is integrated before it is safe.

---

## 5. Previous Round Outstanding P2 Issue Tracking

The following issues were flagged as P2 deferred in Round 11 and remain unfixed in this round:

| ID | Description | Status |
|----|-------------|--------|
| DEV-2 | Compliance module integration with channel operations | Unfixed (re-described as DEV-8 in this round) |
| DEV-4 | Add sequence/root parameters to off-chain `trigger_challenge` | Unfixed (re-described as DEV-9 in this round) |
| PROG-2 | FinalizeSettlement SPL Token CPI transfer | Unfixed (re-described as PROG-7 in this round) |
| PROG-5 | Ed25519 signature verification via instruction introspection | Unfixed (re-described as PROG-8 in this round) |
| TEST-2 | Compliance module and channel integration tests | Unfixed (re-described as TEST-9 in this round) |
| TEST-3 | Multi-hop payment end-to-end tests | Unfixed (re-described as TEST-10 in this round) |

---

## 6. New Issues Summary for This Round

| ID | Category | Severity | Description | File |
|----|----------|----------|-------------|------|
| BUG-18 | BUG | P1 | On-chain verify_htlc missing timelock check | verify_htlc.rs |
| BUG-19 | BUG | P2 | On-chain settle_after_timeout uses >= instead of > | settle_after_timeout.rs:30 |
| BUG-20 | BUG | P2 | On-chain claim cannot verify claim_amount consistency with leaf_hash | claim.rs |
| BUG-21 | BUG | P3 | Route graph fully-connected topology assumption | routing.rs:81 |
| DEV-7 | DEV | P2 | Route scoring uses avg instead of min success_rate | routing.rs:253 |
| DEV-8 | DEV | P2 | Compliance module not integrated with channel operations | channel.rs |
| DEV-9 | DEV | P2 | Off-chain trigger_challenge missing sequence/root | channel.rs:730 |
| DEV-10 | DEV | P2 | On-chain ChannelAccount missing auto_close_slot | state.rs |
| DEV-11 | DEV | P3 | compliance window_slots not implemented | compliance.rs |
| TEST-8 | TEST | P2 | On-chain program has no tests | ignite-pay-program/ |
| TEST-9 | TEST | P2 | Compliance + channel integration tests missing | — |
| TEST-10 | TEST | P2 | Multi-hop end-to-end tests missing | — |
| TEST-11 | TEST | P3 | Non-participant leaf claim rejection test missing | channel_tests.rs |
| TEST-12 | TEST | P3 | compute_hop_amounts overflow test missing | multihop.rs |
| PROG-6 | PROG | P2 | FundChannel does not update leaf_count | fund_channel.rs |
| PROG-7 | PROG | P2 | FinalizeSettlement does not execute CPI transfer | finalize_settlement.rs |
| PROG-8 | PROG | P2 | Ed25519 signature verification placeholders | multiple instructions |
| PROG-9 | PROG | P3 | ChannelAccount space() calculation imprecise | state.rs |
| PROG-10 | PROG | P2 | trigger_challenge updates state without verifying signature | trigger_challenge.rs |
| PROG-11 | PROG | P3 | cooperative_settle signature verification placeholder | cooperative_settle.rs |

---

## 7. Recommended Priority

### Must Fix (P1)
1. **BUG-18**: Add timelock check to on-chain verify_htlc — directly impacts fund safety

### Should Fix Soon (P2)
2. **BUG-19**: On-chain settle_after_timeout strict `>` check
3. **BUG-20**: On-chain claim amount verification mechanism
4. **DEV-7**: Change route scoring to min success_rate
5. **DEV-10**: Add auto_close_slot to on-chain ChannelAccount
6. **PROG-6**: FundChannel update leaf_count
7. **PROG-7/8/10**: On-chain signature verification and CPI transfer implementation

### Can Be Deferred (P3)
8. **BUG-21**: Route graph topology improvement (current simplified model can be used for MVP first)
9. **DEV-11**: compliance window_slots implementation
10. **TEST-11/12**: Add boundary condition tests
11. **PROG-9/11**: Space calculation fix and signature verification improvements
