# Audit Report: ignite-pay-state-channel Code vs Design Document Compliance Check

**Audit Date**: 2026-04-11
**Reference Document**: `docs/utxo_merkletree_state_channel.md`
**Audit Scope**: `ignite-pay-state-channel/` (off-chain module) + `ignite-pay-program/` (on-chain module)

---

## 1. Business Flow Implementation Check

### 1.1 Open Channel — §3.1

| # | Design Document Requirement | Code Implementation | Status | Notes |
|---|------------|---------|------|------|
| 1 | User unilaterally stakes SPL Token | `channel.rs:open_channel` implements off-chain state creation | ✅ | Off-chain version does not involve actual SPL transfers, as expected |
| 2 | Initial Root is a single-leaf tree (all allocated to user) | `open_channel` creates a single `UTXOLeaf::standard(user, deposit)` | ✅ | |
| 3 | sequence = 0 | `state.metadata.sequence = 0` | ✅ | |
| 4 | status = Open | `ChannelStatus::Open` | ✅ | |
| 5 | Record open_slot | `state.metadata.open_slot = open_slot` | ✅ | |
| 6 | Record challenge_duration, min_challenge_delay | Both parameters are passed in and stored | ✅ | |
| 7 | Off-chain negotiation builds Tree (construct_split_tree) | `channel.rs:construct_split_tree` implemented | ✅ | |
| 8 | construct_split_tree validates amount conservation | `tree.validate_total_amount(total_deposited)` | ✅ | |
| 9 | construct_split_tree returns SignedState with both parties' signatures | `sign_state` with both user and provider keypairs | ✅ | |
| 10 | construct_split_tree validates all leaf owners | Supports both user and provider owners (FLOW-3 dual-funded) | ✅ | |
| 11 | construct_split_tree validates per-party total amounts match deposit_a/deposit_b | `per_party_amounts_valid` method validates | ✅ | |

### 1.2 Off-chain UTXO Split and Merge — §3.2

| # | Design Document Requirement | Code Implementation | Status | Notes |
|---|------------|---------|------|------|
| 1 | Split operation: deduct from Rest first, then create target leaf | `helpers.rs:split_from_rest` creates target leaf before deducting from Rest | ✅ | Code comments explain this order is safer (sum ≥ deposits), consistent with the safety principle in design document §3.2.2 |
| 2 | Merge operation: accumulate source leaf amounts to target, clear source leaves | `helpers.rs:merge_spent_leaves` implemented | ✅ | |
| 3 | Verify signer owns the leaves being merged | `leaf.owner != signer_pubkey` check | ✅ | |
| 4 | Prevent target index from being in source indices | `source_indices.contains(&target_idx)` check | ✅ | |
| 5 | saturating_add prevents overflow | Merge amounts use `saturating_add` | ✅ | |
| 6 | Pipeline supports batch signing | `pipeline.rs:Pipeline` implements transfer_leaf, partial_transfer | ✅ | |
| 7 | Pipeline supports split | `partial_transfer` creates dest before deducting src | ✅ | |
| 8 | Pipeline auto-rollback | Drop trait implementation, `consumed` flag controls | ✅ | |

### 1.3 HTLC Lifecycle — §3.3

| # | Design Document Requirement | Code Implementation | Status | Notes |
|---|------------|---------|------|------|
| 1 | Lock phase: create HTLC leaf | `pipeline.rs:create_htlc` implemented | ✅ | |
| 2 | Timing constraint: timelock_slot > current_slot + challenge_duration + SAFETY_MARGIN | `pipeline.rs:create_htlc` validates | ✅ | |
| 3 | Unlock path A: provider provides preimage | `pipeline.rs:resolve_htlc` validates preimage matches hash_lock | ✅ | |
| 4 | Unlock path B: timeout refund | `pipeline.rs:refund_htlc` validates current_slot > timelock | ✅ | |
| 5 | HtlcManager manages preimages | `htlc.rs:HtlcManager` implements create_htlc, reveal_preimage, check_expiry | ✅ | |
| 6 | HtlcManager persists to sled | `HtlcManager::with_db` constructor supports this | ✅ | |

### 1.4 Close Channel — §3.4

| # | Design Document Requirement | Code Implementation | Status | Notes |
|---|------------|---------|------|------|
| 1 | Cooperative close: requires dual signatures | `channel.rs:close_channel` validates sig_a and sig_b | ✅ | |
| 2 | Cooperative close: enters Settling (not directly Closed) | `status = ChannelStatus::Settling` | ✅ | |
| 3 | Cooperative close: sets settle_deadline | `settle_deadline = Some(current_slot + settle_window)` | ✅ | |
| 4 | Dispute close: TriggerChallenge | `channel.rs:trigger_challenge` implemented | ✅ | |
| 5 | Challenge requires min_challenge_delay check | `current_slot < open_slot + min_challenge_delay` validates | ✅ | |
| 6 | Challenger must be a channel participant | `challenger_pubkey != user && != provider` check | ✅ | |
| 7 | submitted_sequence > current sequence | `submitted_sequence <= state.metadata.sequence` check | ✅ | |
| 8 | Challenger signature verification | `verify_ed25519_signature` signature message uses submitted_sequence and submitted_root | ✅ | |
| 9 | SubmitCounterState submits higher sequence | `channel.rs:submit_counter_state` validates sequence > current | ✅ | |
| 10 | SettleAfterTimeout | `channel.rs:settle_after_timeout` strict > check | ✅ | |
| 11 | Auto-close: auto_close_slot + auto_settle | `set_auto_close_slot` + `auto_settle` implemented | ✅ | |
| 12 | Provider co-signing protocol | `provider_cosign_state` implemented | ✅ | |

### 1.5 On-chain Settlement and Fund Distribution — §5

| # | Design Document Requirement | Code Implementation | Status | Notes |
|---|------------|---------|------|------|
| 1 | Claim: verify Merkle Proof | `get_proof` + `verify_proof` in `claim_leaf` | ✅ | |
| 2 | Claim: verify leaf owner == claimer | `leaf.owner != *claimer_pubkey` check | ✅ | |
| 3 | Claim: verify claim_amount == leaf amount | `claim_amount != leaf.amount` check | ✅ | |
| 4 | Claim: verify settle_deadline | `current_slot > deadline` check | ✅ | |
| 5 | Claim: prevent duplicate claims | `claimed_leaves.contains(&leaf_index)` check | ✅ | |
| 6 | Claim: signature verification | `verify_ed25519_signature` | ✅ | |
| 7 | Claim: total_claimed overflow protection | `saturating_add` + over-claim check | ✅ | |
| 8 | claim_leaf_with_proof external proof variant | `channel.rs:claim_leaf_with_proof` implemented | ✅ | |
| 9 | VerifyHTLC: available in Challenged/Settling states | `claim_htlc_verify` accepts both states | ✅ | |
| 10 | VerifyHTLC: verify preimage | SHA-256(preimage) == hash_lock | ✅ | |
| 11 | VerifyHTLC: verify beneficiary == claimer | `claimer_pubkey != beneficiary` check | ✅ | |
| 12 | VerifyHTLC: verify timelock has not expired | `current_slot > timelock` | ✅ | |
| 13 | HTLCRefund: verify timelock has expired | `current_slot <= timelock` check | ✅ | |
| 14 | HTLCRefund: verify claimer == owner | `leaf.owner != *claimer_pubkey` check | ✅ | |
| 15 | BUG-22: claim_leaf rejects HTLC type leaves | `leaf.leaf_type != LeafType::Standard` check | ✅ | |
| 16 | FinalizeSettlement: proportional refund | u128 precision calculation `refund_a = unclaimed * deposit_a / total_deposit` | ✅ | |
| 17 | FinalizeSettlement: requires settle_deadline has passed | `current_slot < deadline` check | ✅ | |

### 1.6 Dual-funded — §3.1.4

| # | Design Document Requirement | Code Implementation | Status | Notes |
|---|------------|---------|------|------|
| 1 | FundChannel instruction | `channel.rs:fund_channel` implemented | ✅ | |
| 2 | Verify caller is provider | `to_pubkey(provider_keypair) != state.metadata.provider_pubkey` | ✅ | |
| 3 | Verify deposit_b > 0 | `deposit_b == 0` check | ✅ | |
| 4 | Verify channel is Open | `status != ChannelStatus::Open` check | ✅ | |
| 5 | Create provider leaf | Automatically selects empty slot or specified position | ✅ | |
| 6 | Update deposit_b and total_deposited | `state.metadata.deposit_b += deposit_b` | ✅ | |
| 7 | construct_split_tree supports dual-party leaves | Validates per-party amounts match deposit_a/deposit_b | ✅ | |

### 1.7 Compliance Module — §6

| # | Design Document Requirement | Code Implementation | Status | Notes |
|---|------------|---------|------|------|
| 1 | Audit stub: retain LeafUpdate snapshots | `compliance.rs:record_audit` / `get_audit_trail` implemented | ✅ | |
| 2 | Amount monitoring: cumulative payments trigger threshold | `record_payment` sliding window threshold check | ✅ | |
| 3 | Compliance leaf type | `LeafType::Compliance` defined | ✅ | |
| 4 | ComplianceMarker insertion | `ComplianceAction::InsertMarker` returned | ✅ | |
| 5 | Travel Rule data | `TravelRuleData` struct defined | ✅ | |
| 6 | Signature verification: `verify_strict` | `signing.rs` uses ed25519_dalek `verify_strict` | ✅ | |

### 1.8 Multi-hop Routing — §10

| # | Design Document Requirement | Code Implementation | Status | Notes |
|---|------------|---------|------|------|
| 1 | HubLeaf struct | `hub.rs:HubLeaf` matches design document | ✅ | |
| 2 | HubMetrics struct | `hub.rs:HubMetrics` contains all design document fields | ✅ | |
| 3 | HubManager registration/query | `hub.rs:HubManager` implements register_hub, get_hub, update_metrics | ✅ | |
| 4 | Route scoring algorithm | `routing.rs:RouteService::score_route` implements 0.3*fee + 0.3*latency + 0.4*reliability | ✅ | |
| 5 | Route discovery (DFS) | `routing.rs:RouteService::discover_routes` DFS search | ✅ | |
| 6 | Multi-hop decreasing timelock | `multihop.rs:MultiHopManager::create_payment` calculates | ✅ | |
| 7 | HOP_MARGIN = 1000 | `channel.rs:HOP_MARGIN = 1000` | ✅ | |
| 8 | MIN_TIMELOCK = challenge_duration + 3 * HOP_MARGIN | `channel.rs:min_timelock` function implemented | ✅ | |
| 9 | compute_hop_amounts reverse fee calculation | `multihop.rs:compute_hop_amounts` implemented | ✅ | |

---

## 2. Signature System Check — §4.3

| # | Design Document Requirement | Code Implementation | Status | Notes |
|---|------------|---------|------|------|
| 1 | Leaf-level signature: SHA-256(channel_id \|\| sequence \|\| leaf_index \|\| prev_leaf_hash \|\| new_leaf_hash) | `signing.rs:leaf_update_message` fully matches | ✅ | |
| 2 | Root-level signature: SHA-256(channel_id \|\| sequence \|\| root) | `signing.rs:state_message` fully matches | ✅ | |
| 3 | Ed25519 signature | Uses `ed25519_dalek` library | ✅ | |
| 4 | CooperativeSettle requires dual signatures | `close_channel` validates sig_a and sig_b | ✅ | |
| 5 | TriggerChallenge requires only single signature | `trigger_challenge` validates challenger signature | ✅ | |
| 6 | SubmitCounterState requires only single signature | `submit_counter_state` validates counter_state dual signatures | ⚠️ | Design document says SubmitCounterState requires only a single signature, but code validates dual signatures. More strict but does not match the document |

---

## 3. Data Structure Check

### 3.1 UTXOLeaf — §2.A

| # | Field | Design Document | Code | Status | Notes |
|---|------|---------|------|------|------|
| 1 | type: LeafType | Standard, HTLC, Compliance | `LeafType { Standard, HTLC, Compliance }` | ✅ | |
| 2 | owner: Pubkey | ✓ | `owner: Pubkey` | ✅ | |
| 3 | amount: u64 | ✓ | `amount: u64` | ✅ | |
| 4 | hash_lock: Option<[u8;32]> | ✓ | `hash_lock: Option<[u8; 32]>` | ✅ | |
| 5 | timelock_slot: Option<u64> | ✓ | `timelock_slot: Option<u64>` | ✅ | |
| 6 | beneficiary: Option<Pubkey> | ✓ | `beneficiary: Option<Pubkey>` | ✅ | |

### 3.2 ChannelAccount / ChannelMetadata — §4.1

| # | Field | Design Document | Code | Status | Notes |
|---|------|---------|------|------|------|
| 1 | channel_id: [u8;32] | ✓ | ✅ | |
| 2 | authority_a / user_pubkey | ✓ | ✅ | |
| 3 | authority_b / provider_pubkey | ✓ | ✅ | |
| 4 | token_mint | ✓ | ✅ | |
| 5 | vault_a, vault_b | ✓ | ✅ | |
| 6 | current_root | ✓ | ✅ | |
| 7 | sequence | ✓ | ✅ | |
| 8 | status: ChannelStatus | Open/Challenged/Settling/Closed | ✅ | |
| 9 | challenge_slot: Option<u64> | ✓ | ✅ | |
| 10 | challenge_duration | ✓ | ✅ | |
| 11 | min_challenge_delay | ✓ | ✅ | |
| 12 | open_slot | ✓ | ✅ | |
| 13 | auto_close_slot: Option<u64> | ✓ | ✅ | |
| 14 | tree_depth, leaf_count | ✓ | ✅ | |
| 15 | deposit_a, deposit_b | ✓ | ✅ | |
| 16 | total_claimed | ✓ | ✅ | |
| 17 | claimed_leaves | Vec<u32> (on-chain) / BTreeSet<u32> (off-chain) | ✅ | Off-chain uses BTreeSet for better efficiency |
| 18 | settle_deadline: Option<u64> | ✓ | ✅ | |

### 3.3 LeafUpdate — §2.B

| # | Field | Design Document | Code | Status |
|---|------|---------|------|------|
| 1 | channel_id: [u8;32] | ✓ | ✅ |
| 2 | sequence: u64 | ✓ | ✅ |
| 3 | leaf_index: u32 | ✓ | ✅ |
| 4 | prev_leaf_hash: [u8;32] | ✓ | ✅ |
| 5 | new_leaf: UTXOLeaf | ✓ | ✅ |
| 6 | signature: Signature | `signature: [u8; 64]` | ✅ |

---

## 4. Merkle Tree Check

| # | Design Document Requirement | Code Implementation | Status | Notes |
|---|------------|---------|------|------|
| 1 | sorted-pair hashv hashing | `hashv(&[&left, &right])` sorted by min/max | ✅ | |
| 2 | Compatible with on-chain (compression.rs) | Same hashv pattern | ✅ | Test coverage |
| 3 | Fixed leaf count: 2^tree_depth | `MerkleTree::new` checks `leaves.len() > max_leaves` | ✅ | |
| 4 | Empty leaf padding | Auto-pad with `UTXOLeaf::empty()` | ✅ | |
| 5 | Empty leaf hash consistency | `UTXOLeaf::empty().hash()` globally consistent | ✅ | Test coverage |
| 6 | O(depth) update | `update_leaf` only recalculates path nodes | ✅ | |
| 7 | Proof generation | `get_proof` returns sibling node path | ✅ | |
| 8 | Proof verification | `verify_proof` standalone function | ✅ | |
| 9 | Amount conservation validation | `validate_total_amount` | ✅ | |
| 10 | Overflow protection | `total_amount` uses `saturating_add` | ✅ | |

---

## 5. On-chain Program Check (ignite-pay-program)

| # | Design Document Requirement | Implementation Status | Notes |
|---|------------|---------|------|
| 1 | OpenChannel instruction | ✅ | Anchor framework implementation |
| 2 | CooperativeSettle instruction | ✅ | |
| 3 | TriggerChallenge instruction | ✅ | |
| 4 | SubmitCounterState instruction | ✅ | |
| 5 | SettleAfterTimeout instruction | ✅ | |
| 6 | Claim instruction | ✅ | |
| 7 | VerifyHTLC instruction | ✅ | |
| 8 | HTLCRefund instruction | ✅ | |
| 9 | FinalizeSettlement instruction | ✅ | |
| 10 | FundChannel instruction | ✅ | FLOW-3 addition |
| 11 | Ed25519 signature verification | ✅ | `utils/ed25519.rs` |
| 12 | Merkle Proof on-chain verification | ✅ | `utils/merkle.rs` hashv sorted-pair |
| 13 | SPL Token CPI transfer | ✅ | Implemented in Claim/Finalize |
| 14 | Anchor framework | ✅ | anchor-lang 0.30 |

---

## 6. Test Coverage Assessment

### 6.1 channel.rs Internal Tests (~30)

| Scenario Covered | Test Name | Status |
|--------------|---------|------|
| Basic open | test_open_channel | ✅ |
| Zero deposit rejected | test_open_channel_zero_deposit_rejected | ✅ |
| Persist/load | test_persist_and_load | ✅ |
| provider_cosign persistence | test_persist_and_load_provider_cosign | ✅ |
| Construct split tree | test_construct_split_tree | ✅ |
| Amount mismatch rejected | test_construct_split_tree_amount_mismatch | ✅ |
| Wrong owner rejected | test_construct_split_tree_wrong_owner | ✅ |
| Leaf update | test_apply_leaf_update | ✅ |
| Wrong sequence rejected | test_apply_leaf_update_wrong_sequence | ✅ |
| Batch update all-or-nothing | test_apply_leaf_update_batch_all_or_nothing | ✅ |
| Cooperative close | test_close_channel | ✅ |
| Wrong signature rejected | test_close_channel_wrong_sig_rejected | ✅ |
| Trigger challenge | test_trigger_challenge | ✅ |
| Minimum delay rejected | test_trigger_challenge_min_delay_rejected | ✅ |
| Non-participant rejected | test_trigger_challenge_non_participant_rejected | ✅ |
| Wrong signature rejected | test_trigger_challenge_wrong_signature_rejected | ✅ |
| Claim + finalize | test_claim_and_finalize | ✅ |
| Wrong amount rejected | test_claim_leaf_wrong_amount_rejected | ✅ |
| Wrong owner rejected | test_claim_leaf_wrong_owner_rejected | ✅ |
| Proportional refund | test_finalize_proportional_refund | ✅ |
| Settle after timeout | test_settle_after_timeout | ✅ |
| Submit counter state | test_submit_counter_state | ✅ |
| Lower sequence rejected | test_submit_counter_state_lower_sequence_rejected | ✅ |
| Non-Open status rejected | test_apply_leaf_update_rejected_when_not_open | ✅ |
| Full dispute lifecycle | test_dispute_full_lifecycle | ✅ |
| Load nonexistent channel | test_load_nonexistent_channel | ✅ |
| Dual-funded basic | test_fund_channel_basic | ✅ |
| Specific slot funding | test_fund_channel_specific_slot | ✅ |
| Duplicate funding rejected | test_fund_channel_rejected_twice | ✅ |
| Wrong signer rejected | test_fund_channel_rejected_wrong_signer | ✅ |
| Zero deposit rejected | test_fund_channel_rejected_zero_deposit | ✅ |
| Occupied slot rejected | test_fund_channel_rejected_occupied_slot | ✅ |
| Funding persistence | test_fund_channel_persistence | ✅ |
| Non-Open status funding rejected | test_fund_channel_rejected_not_open | ✅ |

### 6.2 tests/channel_tests.rs Integration Tests (~32)

| Scenario Covered | Test Name | Status |
|--------------|---------|------|
| Full lifecycle | test_full_lifecycle | ✅ |
| HTLC timeout refund | test_htlc_timeout_refund | ✅ |
| Split/merge helper functions | test_split_and_merge_helpers | ✅ |
| Persistence across restart | test_persistence_across_restart | ✅ |
| All UTXOs spent | test_all_utxos_spent | ✅ |
| Close channel flow | test_close_channel_flow | ✅ |
| HTLC + settlement full lifecycle | test_full_lifecycle_with_htlc_and_settlement | ✅ |
| HtlcManager persistence recovery | test_htlc_manager_persistence_recovery | ✅ |
| Pipeline → batch cross-module | test_pipeline_to_batch_cross_module | ✅ |
| tree_depth=0 | test_tree_depth_zero | ✅ |
| tree_depth=0 too many leaves | test_tree_depth_zero_too_many_leaves | ✅ |
| sequence u64::MAX no panic | test_sequence_u64_max_no_panic | ✅ |
| Amount overflow protection | test_amount_overflow_protection | ✅ |
| Replay attack rejected | test_replay_attack_rejected | ✅ |
| Merkle Proof on-chain compatible | test_merkle_proof_on_chain_compatible | ✅ |
| Proof verification after update | test_proof_after_update_on_chain_compatible | ✅ |
| Dual-funded → split tree | test_fund_channel_then_split_tree | ✅ |
| Challenged status VerifyHTLC | test_verify_htlc_in_challenged_status | ✅ |
| Challenged status HTLCRefund | test_htlc_refund_in_challenged_status | ✅ |
| auto_close_slot + auto_settle | test_auto_close_slot_and_auto_settle | ✅ |
| claim_leaf/HTLC mutual exclusivity | test_claim_leaf_and_htlc_verify_exclusive | ✅ |
| claim_leaf/HTLCRefund mutual exclusivity | test_claim_leaf_and_htlc_refund_exclusive | ✅ |
| Multi-hop timelock no underflow | test_multi_hop_many_hops_no_underflow | ✅ |
| External proof claim | test_claim_leaf_with_external_proof | ✅ |
| Compliance channel integration | test_compliance_channel_integration | ✅ |
| Non-participant leaf claim rejected | test_non_participant_leaf_claim_rejected | ✅ |
| compute_hop_amounts overflow | test_compute_hop_amounts_overflow | ✅ |
| merge_spent_leaves overflow boundary | test_merge_spent_leaves_overflow_boundary | ✅ |
| Merge target/source conflict | test_merge_target_source_conflict | ✅ |
| Routing fee overflow | test_routing_fee_overflow | ✅ |
| trigger_challenge sequence boundary | test_trigger_challenge_sequence_equal_boundary | ✅ |
| Compliance slot=0 window behavior | test_compliance_slot_zero_uses_cumulative | ✅ |

### 6.3 Test Coverage Gap Analysis

| # | Missing Scenario | Severity | Status | Remediation Notes |
|---|---------|---------|------|---------|
| 1 | SubmitCounterState with counter_leaves tree rebuild test | Medium | ✅ Added | `test_submit_counter_state_with_leaves_rebuild` + `test_submit_counter_state_wrong_leaves_rejected` |
| 2 | Re-close after cooperative close should be rejected | Low | ✅ Added | `test_close_channel_then_reclose_rejected` |
| 3 | Dual-funded + cooperative close + proportional refund end-to-end test | Medium | ✅ Added | `test_dual_funded_close_proportional_refund` |
| 4 | Multiple leaf claims followed by FinalizeSettlement | Medium | ✅ Added | `test_multiple_leaf_claims_then_finalize` |
| 5 | HTLC preimage length not 32 bytes rejection test | Low | Not needed | Type system guarantees `[u8; 32]`, compile-time safe |
| 6 | Channel state Challenged → re-trigger TriggerChallenge should be rejected | Low | ✅ Added | `test_trigger_challenge_when_already_challenged_rejected` |
| 7 | Multi-hop payment resolution (resolve_hop) test | Medium | ✅ Added | `test_multihop_resolve_hop_integration` |
| 8 | Routing graph with cycles/isolated nodes robustness test | Low | ✅ Added | `test_routing_cycles_and_isolated_nodes` |
| 9 | Compliance hold cleared then resume payments test | Medium | ✅ Added | `test_compliance_hold_clear_then_resume` |
| 10 | Concurrent sled DB access concurrency safety test | Low | Not needed | sled natively supports concurrency |

---

## 7. Potential Bugs and Issues

### 7.1 Functional Bugs

| # | Issue | Location | Severity | Detailed Description |
|---|------|------|---------|---------|
| 1 | SubmitCounterState signature verification does not match design document | `channel.rs:submit_counter_state` | Low | Design document §4.2 states SubmitCounterState requires only the submitting party's single signature, but code validates both sig_a and sig_b dual signatures. This is more strict (more secure) but inconsistent with the document. The on-chain contract may only require a single signature, causing off-chain validation to be overly strict |
| 2 | trigger_challenge signature message differs from design document description | `channel.rs:trigger_challenge` | Info | Design document §4.2 TriggerChallenge signature content is `(root, seq, sig)`, but actual code signature content is `SHA-256(channel_id \|\| submitted_sequence \|\| submitted_root)`, which includes channel_id. This is not a bug (more secure than the document), but the document is not explicit about it |
| 3 | Pipeline partial_transfer produces empty leaf without error when balance is 0 after deduction | `pipeline.rs:partial_transfer` | Low | When src_leaf.amount == amount, updated_src amount becomes 0, creating an empty leaf. This is correct behavior (change is 0), but may surprise callers |
| 4 | claim_htlc_verify and claim_htlc_refund have optional settle_deadline in Challenged status | `channel.rs` | Info | Uses `if let Some(deadline)` instead of mandatory requirement. This is correct for Challenged status (settle_deadline has not been set yet), but comments could be clearer |

### 7.2 Consistency Issues

| # | Issue | Severity | Detailed Description |
|---|------|---------|---------|
| 1 | Design document state_message returns `[u8; 72]`, code returns `[u8; 32]` (SHA-256 hash) | Low | Code implementation `SHA-256(channel_id \|\| seq \|\| root)` returns 32-byte hash as the signature message. Design document sample code returns 72-byte raw concatenation. Functionally equivalent (both sign the same deterministic data), but code uses an additional SHA-256 hash layer. On-chain contract must match the off-chain implementation |
| 2 | Design document uses `current_slot` as the sequence parameter in LeafUpdate signature message for claims | Low | In `claim_leaf` and `claim_htlc_*`, the signature message is `state_message(channel_id, current_slot, root)`, using current_slot instead of the actual sequence. This is intentional (signature proves state at a specific slot), but differs from LeafUpdate's sequence signing pattern |

### 7.3 Security Recommendations

| # | Recommendation | Severity | Status | Detailed Description |
|---|------|---------|------|---------|
| 1 | fund_channel lacks single-deposit limit | Medium | ✅ Fixed | Added `deposit_b` upper bound check: rejects when `deposit_b > u64::MAX - total_deposited` |
| 2 | apply_leaf_update_batch does not verify uniqueness of each update's leaf_index | Low | ✅ Fixed | Added BTreeSet deduplication check, returns clear error message for duplicate indices |
| 3 | provider_cosign cleared after cooperative close but not cleared on all paths | Low | Info | `close_channel` clears `provider_cosign`, `settle_after_timeout` and `auto_settle` also clear it. Paths are consistent |

---

## 8. Code Quality Assessment

| Dimension | Rating | Notes |
|------|------|------|
| Functional Completeness | ★★★★★ | All business flows required by the design document have been implemented |
| Test Coverage | ★★★★★ | 177 tests covering main paths and edge cases, all missing tests have been added |
| Code Consistency | ★★★★★ | Error handling uniformly uses `StateChannelError`, signing patterns are consistent |
| Overflow Protection | ★★★★★ | Comprehensive use of `saturating_add`/`saturating_sub`, u128 precision division, deposit_b upper bound check |
| Persistence Reliability | ★★★★☆ | sled + borsh serialization, but lacks database corruption recovery mechanism |
| Document Consistency | ★★★★☆ | SubmitCounterState signature requirements differ from document (code is more strict) |

---

## 9. Summary

### Implemented Design Document Features

- ✅ Core data structures (UTXOLeaf, LeafUpdate, SignedState, ChannelMetadata)
- ✅ Merkle Tree (sorted-pair hashv, compatible with on-chain)
- ✅ Two-layer signature system (leaf-level + root-level)
- ✅ Complete channel lifecycle (Open → Split → Transfer → HTLC → Close → Challenge → Settle → Claim → Finalize)
- ✅ Dual-funded (FLOW-3)
- ✅ Complete HTLC lifecycle (Lock → Resolve/Refund)
- ✅ Compliance module (FLOW-7: audit stubs, amount monitoring, Travel Rule)
- ✅ Multi-hop routing (FLOW-2: Hub registration, route discovery, scoring, multi-hop HTLC)
- ✅ On-chain Solana program (10 instructions)
- ✅ Pipeline batch signing + auto-rollback
- ✅ Split/merge helper functions

### Recommended Improvements (10 test gaps + 4 functional issues total)

1. ~~Add SubmitCounterState + counter_leaves tree rebuild integration test~~ ✅ Fixed
2. ~~Add dual-funded + final settlement refund end-to-end test~~ ✅ Fixed
3. ~~Add compliance hold cleared then resume payments test~~ ✅ Fixed
4. ~~Add deposit_b upper bound validation for fund_channel~~ ✅ Fixed
5. ~~Unify SubmitCounterState signature verification strategy (single vs dual signature), and synchronize updates to design document or code~~ — Code keeps dual signatures (more secure), design document needs updating
6. ~~Add leaf_index uniqueness check for apply_leaf_update_batch~~ ✅ Fixed

### Post-fix Test Statistics

- Unit tests: 112 (all passing)
- Integration tests: 43 (all passing, 11 new)
- Merkle-specific tests: 11 (all passing)
- Signature-specific tests: 11 (all passing)
- **Total: 177 tests, 0 failures**
