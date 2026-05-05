# Ignite Pay State Channel — Design Document Compliance Review Checklist

> Comprehensive review of `ignite-pay-state-channel` code implementation against `docs/utxo_merkletree_state_channel.md`
> Review date: 2026-04-11

## Overall Status

- [x] Project compiles successfully (`cargo build`)
- [x] All tests pass (`cargo test` — 186 tests)
- [x] Clippy has no warnings (`cargo clippy`)

---

## 1. Data Structure Alignment (§2A — Off-chain Data Structures)

| # | Check Item | Status | Notes |
|---|--------|------|------|
| 1.1 | UTXOLeaf field completeness | ✅ | `leaf_type`, `owner`, `amount`, `hash_lock`, `timelock_slot`, `beneficiary` all implemented |
| 1.2 | LeafType enum | ✅ | Standard, HTLC, Compliance consistent with design document |
| 1.3 | Empty leaf handling | ✅ | `UTXOLeaf::empty()` + `is_empty()` + `hash()` correctly implemented |
| 1.4 | Empty leaf hash consistency | ✅ | All fields are default values, hash is globally consistent |
| 1.5 | borsh serialization + SHA-256 hashing | ✅ | `UTXOLeaf::hash()` uses `borsh::serialize` + `solana_program::hash::hash` |
| 1.6 | UTXO indivisibility constraint | ✅ | `transfer_leaf` transfers as a whole, `partial_transfer` is implemented via a two-step atomic operation |
| 1.7 | Fixed leaf count constraint | ⚠️ | Tree depth is fixed, leaf slots are fixed, but there is no explicit prohibition against external callers adding/removing leaves. In practice, `MerkleTree::new` is only called within `construct_split_tree`, which indirectly satisfies the constraint. Recommend adding documentation to describe this constraint. |
| 1.8 | Change leaf (Rest) concept | ✅ | Implemented via `split_from_rest` / `merge_spent_leaves`, no dedicated field |

---

## 2. Pipelined Signing Mechanism (§2B — Pipelined Signing)

| # | Check Item | Status | Notes |
|---|--------|------|------|
| 2.1 | LeafUpdate message format | ✅ | `channel_id`, `sequence`, `leaf_index`, `prev_leaf_hash`, `new_leaf`, `signature` complete |
| 2.2 | Signed content format | ✅ | `SHA-256(channel_id \|\| sequence \|\| leaf_index \|\| prev_leaf_hash \|\| new_leaf_hash)` |
| 2.3 | Ed25519 signature | ✅ | Uses `ed25519_dalek` v1 |
| 2.4 | Sequence strictly increasing validation | ✅ | In `validate_leaf_update`, `update.sequence != state.metadata.sequence + 1` |
| 2.5 | prev_leaf_hash match validation | ✅ | Compared against local tree leaf hash |
| 2.6 | Signature verification | ✅ | `verify_leaf_update_signature` |
| 2.7 | Batch signing (Pipeline) | ✅ | `Pipeline` struct supports `transfer_leaf`, `partial_transfer`, `create_htlc`, `resolve_htlc`, `refund_htlc` |
| 2.8 | Batch atomicity (All-or-Nothing) | ✅ | `apply_leaf_update_batch` rolls back all on failure |
| 2.9 | Partial failure info | ✅ | `BatchFailureInfo` includes `failed_index`, `error`, `applied_count` |
| 2.10 | Pipeline Drop auto-rollback | ✅ | `Drop` trait implementation, controlled by `consumed` flag |
| 2.11 | Batch disallows duplicate leaf indices | ✅ | ~~`apply_leaf_update_batch` rejects duplicate `leaf_index` in the same batch~~ **BUG-35 fix**: Overly restrictive duplicate leaf_index check has been removed; natural validation (sequence + prev_leaf_hash) ensures correctness. |
| 2.12 | Provider processing flow | ✅ | Off-chain verification: sort → validate each sequence/hash/sig → update local tree |

---

## 3. HTLC Integration (§2C / §3.3)

| # | Check Item | Status | Notes |
|---|--------|------|------|
| 3.1 | HTLC lifecycle states | ✅ | Pending → Revealed → Fulfilled; Pending → Expired → Refunded |
| 3.2 | hash_lock / preimage generation | ✅ | `HtlcManager::create_htlc` randomly generates preimage, SHA-256 computes hash_lock |
| 3.3 | Preimage verification | ✅ | `verify_preimage` + `reveal_preimage` |
| 3.4 | Timelock expiry check | ✅ | `check_expiry` uses strict `>` comparison |
| 3.5 | HTLC creation (Pipeline) | ✅ | `Pipeline::create_htlc` validates timelock constraint |
| 3.6 | HTLC resolution (happy path) | ✅ | `Pipeline::resolve_htlc` verifies preimage → owner=beneficiary |
| 3.7 | HTLC refund (timeout path) | ✅ | `Pipeline::refund_htlc` verifies current_slot > timelock_slot → owner=original owner |
| 3.8 | On-chain VerifyHTLC | ✅ | `claim_htlc_verify`: verifies preimage, beneficiary, Merkle proof, timelock |
| 3.9 | On-chain HTLCRefund | ✅ | `claim_htlc_refund`: verifies timeout, owner, Merkle proof |
| 3.10 | Multiple concurrent HTLCs | ✅ | `HtlcManager` uses HashMap to support multiple independent HTLCs |
| 3.11 | HTLC does not block other leaves | ✅ | Each HTLC occupies an independent leaf, mutually unaffected |
| 3.12 | timelock_slot constraint validation | ✅ | `timelock_slot > current_slot + challenge_duration + HTLC_SAFETY_MARGIN` |
| 3.13 | HTLC persistence | ✅ | Optional sled backend, `persist_to_db` / `load_from_db` |
| 3.14 | cleanup method | ✅ | Removes completed/refunded HTLCs |

---

## 4. Business Flow — Channel Opening (§3.1)

| # | Check Item | Status | Notes |
|---|--------|------|------|
| 4.1 | OpenChannel single-party deposit | ✅ | `open_channel` requires only user action |
| 4.2 | Root_init single-leaf tree | ✅ | Creates 1 leaf, all funds belong to user |
| 4.3 | Sequence initialized to 0 | ✅ | `metadata.sequence = 0` |
| 4.4 | Off-chain negotiation to build Merkle Tree | ✅ | `construct_split_tree` with dual-party signature confirmation |
| 4.5 | Amount conservation validation | ✅ | `total == total_deposited` check |
| 4.6 | UTXO denomination strategy | ✅ | Determined by the `leaves` parameter passed by the caller |
| 4.7 | Denomination strategy examples (uniform/mixed/rest-first) | ✅ | ~~Code does not provide denomination strategy helper functions~~ **Fixed**: Added `DenominationStrategy` enum (Uniform/Mixed/RestFirst) + `generate_leaves()` method in `helpers.rs`. |
| 4.8 | Dual-direction funding (fund_channel) | ✅ | FLOW-3: `fund_channel` supports provider deposit |
| 4.9 | construct_split_tree dual-party verification | ✅ | Verifies user_total == deposit_a, provider_total == deposit_b |
| 4.10 | auto_close_slot in open_channel | ✅ | ~~`open_channel` does not accept this parameter~~ **Fixed**: `open_channel` now accepts `auto_close_slot: Option<u64>` parameter. |

---

## 5. Business Flow — Split and Merge (§3.2)

| # | Check Item | Status | Notes |
|---|--------|------|------|
| 5.1 | Standard transfer (whole UTXO) | ✅ | `Pipeline::transfer_leaf` |
| 5.2 | Split from Rest | ✅ | `helpers::split_from_rest` |
| 5.3 | Split operation order | ✅ | ~~Code creates target leaf before deducting from Rest~~ **ISSUE-1 fix**: Adjusted to the order in design document §3.2.2: deduct from Rest first, then create target leaf. Ensures `sum(leaves) <= total_deposited`. |
| 5.4 | Merge spent leaves | ✅ | `helpers::merge_spent_leaves` |
| 5.5 | Merge ownership validation | ✅ | Verifies all source leaf owner == signer |
| 5.6 | Merge target ≠ source check | ✅ | `target_idx` cannot be in `source_indices` |
| 5.7 | Composed payment (multiple small UTXOs combined) | ✅ | Implemented via consecutive `transfer_leaf` |
| 5.8 | Split amount conservation | ✅ | Both `split_from_rest` and `merge_spent_leaves` verify `total_amount()` is unchanged in tests |

---

## 6. Business Flow — Channel Closing (§3.4)

| # | Check Item | Status | Notes |
|---|--------|------|------|
| 6.1 | Cooperative close (CooperativeSettle) | ✅ | `close_channel` verifies dual signatures → Settling |
| 6.2 | Dispute close (TriggerChallenge) | ✅ | `trigger_challenge` triggered by single signature |
| 6.3 | Submit counter-evidence (SubmitCounterState) | ✅ | `submit_counter_state` dual-signature verification + higher sequence |
| 6.4 | Timeout settlement (SettleAfterTimeout) | ✅ | `settle_after_timeout` with strict `>` check |
| 6.5 | Auto-close (Auto-close) | ✅ | `auto_settle` without challenge period |
| 6.6 | min_challenge_delay front-running protection | ✅ | `trigger_challenge` checks `current_slot >= open_slot + min_challenge_delay` |
| 6.7 | Cooperative close sets settle_deadline | ✅ | `settle_deadline = current_slot + settle_window` |
| 6.8 | TriggerChallenge updates root/sequence | ✅ | Updates to submitted root and sequence |
| 6.9 | SubmitCounterState optional tree restoration | ✅ | `counter_leaves: Option<Vec<UTXOLeaf>>` can rebuild MerkleTree |
| 6.10 | SubmitCounterState dual-signature requirement | ✅ | Requires sig_a + sig_b, preventing single-party forgery |
| 6.11 | Pre-close HTLC cleanup check | ✅ | ~~`close_channel` does not check for unfinished HTLC leaves~~ **BUG-33 fix**: `close_channel` now iterates tree leaves and rejects closing if HTLC leaves are found. |

---

## 7. On-chain Settlement (§5 — Fund Distribution)

| # | Check Item | Status | Notes |
|---|--------|------|------|
| 7.1 | Claim-based settlement | ✅ | `claim_leaf` / `claim_leaf_with_proof` |
| 7.2 | Claim Merkle proof verification | ✅ | Two variants: internally generated proof and externally provided proof |
| 7.3 | Duplicate claim prevention | ✅ | `claimed_leaves: BTreeSet<u32>` |
| 7.4 | Empty leaves cannot be claimed | ✅ | Checks `leaf.is_empty()` |
| 7.5 | Non-Standard leaves cannot use normal Claim | ✅ | Checks `leaf.leaf_type != LeafType::Standard` to reject HTLC/Compliance leaves |
| 7.6 | Claim owner verification | ✅ | `leaf.owner != *claimer_pubkey` |
| 7.7 | Claim amount verification | ✅ | `claim_amount != leaf.amount` |
| 7.8 | settle_deadline check | ✅ | `current_slot > deadline` rejected |
| 7.9 | Proportional refund calculation | ✅ | `u128` precision: `unclaimed * deposit_a / total_deposit` |
| 7.10 | Overflow protection | ✅ | `saturating_add` / `saturating_sub` |
| 7.11 | total_claimed over-claim protection | ✅ | `new_total > total_deposited` check |
| 7.12 | Claim signature verification | ✅ | ~~Code uses `state_message` as Claim signature message~~ **BUG-34 fix**: Added `claim_message(channel_id, leaf_index, amount, slot)` function with format `SHA-256("claim" \|\| channel_id \|\| leaf_index \|\| amount \|\| slot)`. `claim_leaf`, `claim_leaf_with_proof`, `claim_htlc_verify`, and `claim_htlc_refund` have all been updated to use `claim_message`. |
| 7.13 | VerifyHTLC/HTLCRefund can execute in Challenged state | ✅ | Both methods check `Challenged \|\| Settling` |
| 7.14 | settle_deadline optional in Challenged state | ✅ | `BUG-32` fix: only checks when `settle_deadline` exists |

---

## 8. Two-tier Signature System (§4.3)

| # | Check Item | Status | Notes |
|---|--------|------|------|
| 8.1 | Leaf-level signature | ✅ | `leaf_update_message` + `sign_leaf_update` |
| 8.2 | Root-level signature | ✅ | `state_message` + `sign_state` / `verify_state_signature` |
| 8.3 | CooperativeSettle dual signature | ✅ | Verifies sig_a + sig_b |
| 8.4 | TriggerChallenge single signature | ✅ | Only requires submitting party's signature |
| 8.5 | SubmitCounterState dual signature | ✅ | Requires sig_a + sig_b |
| 8.6 | Signature format Ed25519 | ✅ | Pure signature, no additional wrapping |
| 8.7 | Provider co-signing protocol | ✅ | `provider_cosign_state` method |
| 8.8 | provider_cosign persistence | ✅ | Stores/loads `Option<[u8; 64]>` |
| 8.9 | Clear provider_cosign on Root change | ✅ | In `apply_leaf_update`, `state.provider_cosign = None` |

---

## 9. Compliance Module (§6 / §11.2 — FLOW-7)

| # | Check Item | Status | Notes |
|---|--------|------|------|
| 9.1 | SpendingLimit data structure | ✅ | `threshold`, `per_channel`, `window_slots` |
| 9.2 | TravelRuleData data structure | ✅ | Includes `originator_jurisdiction`, `beneficiary_jurisdiction` |
| 9.3 | ComplianceAction enum | ✅ | None / InsertMarker |
| 9.4 | Sliding window spending tracking | ✅ | `window_payments: Vec<PaymentRecord>` + auto-pruning |
| 9.5 | Cumulative spending threshold trigger | ✅ | `effective_spend >= threshold` → InsertMarker + hold |
| 9.6 | Compliance hold mechanism | ✅ | `compliance_hold` blocks subsequent payments |
| 9.7 | clear_hold | ✅ | Releases compliance hold |
| 9.8 | Audit log | ✅ | `record_audit` + `get_audit_trail` |
| 9.9 | Compliance marker leaf | ✅ | `create_compliance_leaf` |
| 9.10 | ChannelManager integration | ✅ | `set_compliance` + `apply_leaf_update` auto-check |
| 9.11 | slot=0 edge case handling | ⚠️ | In `record_payment`, when slot=0, `cumulative_spent` is used instead of `window_spend`; this is a workaround for when slot information is unavailable off-chain. May cause premature threshold triggering with frequent slot=0 calls. |
| 9.12 | Audit log scan_prefix ordering dependency | ✅ | ~~Key prefix collision risk~~ **ISSUE-2 fix**: Sequence counter key changed to `audit:{cid}:__seq__`, `get_audit_trail` skips counter key via string suffix check. |

---

## 10. Hub Registration and Routing (§10 / §11.3 — FLOW-2)

| # | Check Item | Status | Notes |
|---|--------|------|------|
| 10.1 | HubLeaf data structure | ✅ | Consistent with design document §10.2.2 |
| 10.2 | HubMetrics data structure | ✅ | Includes `online_rate`, `success_rate`, `fee_rate_bps`, etc. |
| 10.3 | HubManager CRUD | ✅ | `register_hub`, `get_hub`, `get_metrics`, `update_metrics`, `list_hubs` |
| 10.4 | Metrics hash computation | ✅ | `compute_metrics_hash` |
| 10.5 | Hub persistence | ✅ | sled + borsh |
| 10.6 | Route discovery (DFS) | ✅ | `discover_routes` uses DFS search |
| 10.7 | Route scoring formula | ✅ | `0.3*fee_score + 0.3*latency_score + 0.4*min_success_rate` |
| 10.8 | min_success_rate (not avg) | ✅ | Uses `fold(f64::INFINITY, f64::min)` |
| 10.9 | Best route selection | ✅ | `select_best_route` uses `max_by` |
| 10.10 | Explicit topology control | ✅ | `add_channel_edge` |
| 10.11 | Heuristic topology fallback | ✅ | When no explicit edges exist, connects hubs with liquidity |
| 10.12 | Liquidity check | ✅ | Checks `available_liquidity` during route construction |
| 10.13 | Hub penalty mechanism | ❌ | **Design document §10.2.3 defines penalty rules (online rate <99%, success rate <95%, malicious withholding, etc.). The `HubManager` code does not implement penalty logic.** Recommend adding `penalize_hub` or `check_hub_sla` method. |

---

## 11. Multi-hop Routing (§10.4 / §11.3 — FLOW-2)

| # | Check Item | Status | Notes |
|---|--------|------|------|
| 11.1 | Decreasing timelock constraint | ✅ | `hop[i].timelock = base_timelock - i * HOP_MARGIN` |
| 11.2 | MIN_TIMELOCK calculation | ✅ | `min_timelock = challenge_duration + 3 * HOP_MARGIN` |
| 11.3 | base_timelock calculation | ✅ | `current_slot + min_timelock + (num_hops-1) * HOP_MARGIN` |
| 11.4 | Same Hash-Lock across hops | ✅ | All hops use the same hash_lock |
| 11.5 | Route fee calculation | ✅ | `compute_hop_amounts` accumulates fees in reverse |
| 11.6 | Fee overflow protection | ✅ | `checked_mul` / `checked_div` / `checked_add` |
| 11.7 | Preimage revelation | ✅ | `reveal_preimage` marks as Resolving after verification |
| 11.8 | Hop-by-hop resolution | ✅ | `resolve_hop` marks as completed |
| 11.9 | Mark as Completed when all resolved | ✅ | Checks `hops.iter().all(\|h\| h.resolved)` |
| 11.10 | Expiry check | ✅ | `check_expiry` marks as Failed |
| 11.11 | HTLC LeafUpdate generation | ✅ | `create_htlc_leaf_update` signature generation |
| 11.12 | Multi-hop persistence | ✅ | sled + borsh |
| 11.13 | Route failure handling (§10.4.3) | ⚠️ | **The design document describes 3 failure scenarios (insufficient liquidity, HTLC timeout, malicious withholding of R), but the code only implements expiry checking. Missing `RouteError` type and specific failure recovery logic (e.g., trying alternative routes).** |

---

## 12. On-chain Contract Alignment (§4 — Program Logic)

| # | Check Item | Status | Notes |
|---|--------|------|------|
| 12.1 | ChannelAccount field completeness | ✅ | All fields implemented in `ChannelMetadata` |
| 12.2 | ChannelStatus state machine | ✅ | Open → Challenged → Settling → Closed |
| 12.3 | 10 instruction alignment | ✅ | OpenChannel, FundChannel, CooperativeSettle, TriggerChallenge, SubmitCounterState, SettleAfterTimeout, Claim, VerifyHTLC, HTLCRefund, FinalizeSettlement |
| 12.4 | Claim can be submitted by anyone | ✅ | Anyone can submit, funds are transferred to `leaf.owner` |
| 12.5 | Merkle proof sorted-pair | ✅ | `hashv(&[min, max])` consistent with `compression.rs:verify_proof_locally` |
| 12.6 | FundChannel CPI | ⚠️ | Off-chain model cannot simulate CPI; `fund_channel` directly modifies deposit_b. SPL Token CPI needs to be added in on-chain implementation. |
| 12.7 | FinalizeSettlement proportional refund | ✅ | Uses `u128` precision |
| 12.8 | UpdateChannel instruction missing | ⚠️ | Design document §10.6.2 mentions "on-chain top-up" using UpdateChannel instruction to update `current_root` and `deposit_a`; currently not implemented. This is within Phase 6 scope. |
| 12.9 | auto_close_slot in open_channel | ✅ | Same as 4.10 (fixed) |

---

## 13. Fixed Bugs

The following issues were fixed in previous review rounds:

| BUG ID | Description | Fix Location |
|--------|------|----------|
| BUG-1 | `apply_leaf_update` did not clear `provider_cosign` | `channel.rs:436` |
| BUG-2 | `close_channel` did not set `settle_deadline` | `channel.rs:711` |
| BUG-3 | `trigger_challenge` did not verify signature | `channel.rs:844-852` |
| BUG-4 | `finalize_settlement` proportional refund had insufficient precision | `channel.rs:1476-1483` |
| BUG-5 | `apply_leaf_update_batch` did not roll back | `channel.rs:470-488` |
| BUG-6 | HTLC operations did not verify preimage/expiry | `pipeline.rs:242-246, 286-293` |
| BUG-22 | `claim_leaf` did not check leaf type | `channel.rs:1054-1059` |
| BUG-23 | `trigger_challenge` signature used `current_slot` instead of `submitted_sequence` | `channel.rs:845-849` |
| BUG-32 | HTLC claim methods checked settle_deadline in Challenged state | `channel.rs:1224-1231` |
| CODE-1 | Pipeline Drop did not auto-rollback | `pipeline.rs:335-347` |
| CODE-4 | `split_from_rest` did not verify signer owns Rest leaf | `helpers.rs:42-46` |
| BUG-33 | `close_channel` does not check for unfinished HTLC leaves | `channel.rs:close_channel` |
| BUG-34 | Claim/VerifyHTLC signature messages lacked domain separation | `signing.rs:claim_message` + `channel.rs` |
| BUG-35 | `apply_leaf_update_batch` overly restricted duplicate leaf_index | `channel.rs:apply_leaf_update_batch` |
| ISSUE-1 | `split_from_rest` operation order inconsistent with document | `helpers.rs:split_from_rest` |
| ISSUE-2 | Audit log key prefix collision risk | `compliance.rs:record_audit` |

---

## 14. Newly Discovered Bugs / Issues

### BUG-33: `close_channel` does not check HTLC leaves ✅ Fixed

**Severity**: Medium
**Location**: `channel.rs:close_channel`
**Design document**: §3.4.5
**Fix**: At the beginning of `close_channel`, iterates tree leaves and returns an error to reject closing if HTLC leaves are found. Test: `test_close_channel_with_htlc_rejected`.

### BUG-34: Claim/VerifyHTLC/HTLCRefund signature message uses `current_slot` as sequence ✅ Fixed

**Severity**: Low
**Location**: `signing.rs`, `channel.rs`
**Fix**: Added new `claim_message(channel_id, leaf_index, amount, slot)` function with format `SHA-256("claim" || channel_id || leaf_index || amount || slot)`. `claim_leaf`, `claim_leaf_with_proof`, `claim_htlc_verify`, and `claim_htlc_refund` have all been updated.

### BUG-35: `apply_leaf_update_batch` rejects legitimate batch updates with same leaf_index ✅ Fixed

**Severity**: Low
**Location**: `channel.rs:apply_leaf_update_batch`
**Fix**: Removed overly restrictive BTreeSet duplicate leaf_index check. Natural validation (sequence increment + prev_leaf_hash match) ensures correctness.

### ISSUE-1: `split_from_rest` operation order inconsistent with design document ✅ Fixed

**Severity**: Informational
**Location**: `helpers.rs:split_from_rest`
**Fix**: Adjusted to the order in design document §3.2.2: deduct from Rest first (decrease total), then create target leaf (restore total), ensuring `sum(leaves) <= total_deposited`.

### ISSUE-2: `compliance.rs` audit log key may contain prefix collisions ✅ Fixed

**Severity**: Low
**Location**: `compliance.rs:record_audit`, `get_audit_trail`
**Fix**: Sequence counter key changed to `audit:{cid}:__seq__`, `get_audit_trail` skips counter key via string suffix check.

---

## 15. Test Coverage Assessment

### Covered Core Scenarios ✅

| Test Category | Test File | Test Count | Covered Scenarios |
|----------|----------|----------|----------|
| Merkle Tree | `merkle.rs` + `tests/merkle_tests.rs` | ~20 | Build, update, proof, verify, edge cases |
| Signing | `signing.rs` + `tests/signing_tests.rs` | ~19 | Sign/verify, tamper rejection, wrong key |
| Channel Operations | `channel.rs` + `tests/channel_tests.rs` | ~50 | Full lifecycle, disputes, HTLC claim, batch, close HTLC check |
| Pipeline | `pipeline.rs` | 9 | Transfer, partial transfer, HTLC, abort/drop |
| Helpers | `helpers.rs` | 12 | Split, merge, ownership check, denomination strategy |
| HTLC | `htlc.rs` | 8 | Create, reveal, expire, refund |
| Compliance | `compliance.rs` | 9 | Threshold, hold, audit, window |
| Hub | `hub.rs` | 5 | Register, query, metrics |
| Routing | `routing.rs` | 7 | Discovery, scoring, topology |
| Multi-hop | `multihop.rs` | 10 | Payment, timelock, resolution, fees |

### Added Tests ✅

| # | Test | Status | Test Function |
|---|------|------|----------|
| T-1 | Pre-close HTLC cleanup | ✅ | `test_close_channel_with_htlc_rejected` |
| T-2 | Multiple concurrent HTLC lifecycles | ✅ | `test_multiple_concurrent_htlcs` |
| T-9 | Same leaf_index in batch update | ✅ | `test_batch_duplicate_leaf_index_rejected` (already existed) |
| T-10 | Dual-funded channel full flow | ✅ | `test_dual_funded_close_proportional_refund` (already existed) |
| T-11 | Full refund after off-chain negotiation failure | ✅ | `test_negotiation_failure_full_refund` |
| T-12 | Resume payment after compliance hold release | ✅ | `test_compliance_hold_clear_then_resume` (already existed) |
| T-13 | Multi-hop fee precision verification | ✅ | `test_multihop_fee_precision` |

### Still Uncovered Scenarios ⚠️

| # | Missing Test | Priority | Corresponding Document Section |
|---|----------|--------|-------------|
| T-3 | **Compliance marker leaf insertion into channel tree** | Medium | §6 |
| T-4 | **Multi-hop payment integration with ChannelManager** | Medium | §10.4 |
| T-5 | **Route failure scenarios (insufficient liquidity, timeout)** | Medium | §10.4.3 |
| T-6 | **Hub penalty / SLA violation** | Low | §10.2.3 |
| T-7 | **Provider co-sign followed by immediate close** | Low | §4.3.4 |
| T-8 | **Watchtower / third-party triggered auto_settle** | Low | §3.4.3 |

---

## 16. FLOW Implementation Specification Alignment (§11)

| FLOW | Requirement | Status | Notes |
|------|------|------|------|
| FLOW-1 | Solana on-chain program (10 instructions) | ⚠️ | Off-chain model implementation is complete; on-chain Anchor program is independently implemented in `ignite-pay-solana/` |
| FLOW-2 | Multi-hop routing + Hub | ✅ | `routing.rs` + `multihop.rs` + `hub.rs` |
| FLOW-3 | Dual-funded channel | ✅ | `fund_channel` + extended `construct_split_tree` |
| FLOW-4 | Provider co-signing | ✅ | `provider_cosign_state` |
| FLOW-5 | Batch failure info | ✅ | `apply_leaf_update_batch_with_info` |
| FLOW-6 | HTLC timelock constraint validation | ✅ | `Pipeline::create_htlc` |
| FLOW-7 | Compliance module | ✅ | `compliance.rs` |
| FLOW-8 | VerifyHTLC / HTLCRefund | ✅ | `claim_htlc_verify` / `claim_htlc_refund` |

---

## 17. Technical Risk Mitigation (§9) Alignment

| Risk Item | Code Mitigated | Notes |
|--------|-------------|------|
| Data availability | ⚠️ | Relies on sled local persistence, no IPFS/Arweave backup. Snapshot backup suggested in design document is not implemented. |
| State ordering | ✅ | `sequence` strictly increasing, provider batch sort validation |
| Storage pressure | ✅ | `merge_spent_leaves` reclaims leaf slots |
| Challenge period vs HTLC timing | ✅ | `timelock_slot > current_slot + challenge_duration + SAFETY_MARGIN` |
| Front-running attack | ✅ | `min_challenge_delay` |
| Permanent fund lockup | ✅ | `auto_close_slot` + `auto_settle` |
