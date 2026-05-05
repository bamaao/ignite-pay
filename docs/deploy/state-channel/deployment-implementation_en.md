# State Channel Implementation Document

## 1. Overview

The Ignite Pay state channel system implements an off-chain payment channel based on UTXO + Merkle Tree. This document describes the implementation steps to deploy the complete system from scratch.

**Components involved**:
- `ignite-pay-state-channel`: Off-chain Rust library (channel management, Merkle tree, HTLC, routing)
- `ignite-pay-program`: On-chain Solana program (Anchor framework, settlement/dispute handling)

---

## 2. System Architecture Overview

```
┌─────────────┐   LeafUpdate + CoSign   ┌──────────────┐
│  User (A)    │ ←──────────────────────→ │  Merchant/Hub (B) │
│             │                           │              │
│  ChannelMgr │   SignedState (dual-sig)  │  ChannelMgr  │
│  Pipeline   │ ←──────────────────────→ │  CoSign      │
│  HtlcMgr    │                           │  HtlcMgr     │
│  sled DB    │                           │  sled DB     │
└──────┬──────┘                           └──────┬───────┘
       │                                         │
       │         On-chain Settlement (Solana)    │
       └──────────────┬──────────────────────────┘
                      ▼
              ┌──────────────┐
              │ ignite-pay-  │
              │   program    │
              │              │
              │ PDA: channel │
              │ PDA: escrow  │
              └──────────────┘
```

---

## 3. Off-chain Library: ignite-pay-state-channel

### 3.1 Module Structure

| Module | File | Description |
|:-------|:-----|:------------|
| `channel` | `channel.rs` | `ChannelManager` — Channel lifecycle management, sled persistence |
| `merkle` | `merkle.rs` | `MerkleTree` — Sorted-pair hash binary tree |
| `types` | `types.rs` | `UTXOLeaf`, `LeafUpdate`, `SignedState`, `ChannelMetadata` |
| `signing` | `signing.rs` | Ed25519 signing/verification, message construction |
| `pipeline` | `pipeline.rs` | `Pipeline` — Batch LeafUpdate builder with automatic rollback |
| `htlc` | `htlc.rs` | `HtlcManager` — HTLC preimage/lifecycle management |
| `hub` | `hub.rs` | `HubManager` — Hub registration/metrics, sled persistence |
| `routing` | `routing.rs` | `RouteService` — DFS route discovery/scoring |
| `multihop` | `multihop.rs` | `MultiHopManager` — Multi-hop payments, decreasing timelock |
| `compliance` | `compliance.rs` | `ComplianceManager` — Spending limits/audit |
| `error` | `error.rs` | `StateChannelError` unified error type |
| `helpers` | `helpers.rs` | Helper utility functions |

### 3.2 Dependencies

```toml
[dependencies]
solana-program = "2"           # Solana core types (no OpenSSL dependency)
solana-pubkey = "2"            # Pubkey type
ed25519-dalek = "1"            # Ed25519 signing
borsh = "1"                    # Serialization
serde = { version = "1", features = ["derive"] }
sled = "0.34"                  # Embedded database
anyhow = "1"                   # Error handling
rand = "0.7"                   # Random number generation
hex = "0.4"                    # Hex encoding/decoding
tracing = "0.1"                # Logging

[dev-dependencies]
tempfile = "3"                 # Temporary directories for tests
```

### 3.3 Build

```bash
cd ignite-pay-state-channel
cargo build
cargo test
```

### 3.4 Key Constants

| Constant | Value | Description |
|:---------|:------|:------------|
| `HTLC_SAFETY_MARGIN` | 1000 slots | HTLC timelock safety margin (~6.7 minutes) |
| `HOP_MARGIN` | 1000 slots | Multi-hop timelock decrement step (~6.7 minutes) |
| Max `tree_depth` | 12 | On-chain program limit, up to 4096 leaves |

---

## 4. On-chain Program: ignite-pay-program

### 4.1 Program Information

| Attribute | Value |
|:----------|:------|
| Program ID | `DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe` |
| Framework | Anchor 1.0.0 |
| SPL Token | anchor-spl 1.0.0 |

### 4.2 Instruction List

| # | Instruction | Description | Signature Requirements |
|:--|:------------|:------------|:-----------------------|
| 1 | `open_channel` | Create channel PDA + initial root | User signature |
| 2 | `fund_channel` | Provider injects funds | Provider signature |
| 3 | `cooperative_settle` | Dual signature → enter settlement window | User + Provider |
| 4 | `trigger_challenge` | Single party initiates dispute | User or Provider |
| 5 | `submit_counter_state` | Submit updated dual-signed state | Verify sig_a + sig_b |
| 6 | `settle_after_timeout` | Enter settlement after challenge_duration expires | Anyone |
| 7 | `claim` | Submit Merkle Proof to claim standard leaf | Leaf owner |
| 8 | `verify_htlc` | Submit preimage + Merkle Proof to claim HTLC leaf | Beneficiary |
| 9 | `htlc_refund` | Refund HTLC funds after timelock expires | Leaf owner |
| 10 | `finalize_settlement` | Settlement window closes, distribute unclaimed funds | User or Provider |

### 4.3 PDA Accounts

| Account | Seeds | Description |
|:--------|:------|:------------|
| `ChannelAccount` | `["channel", channel_id]` | Channel state |
| `Escrow Vault` | `["escrow", channel_id]` | Escrow Token account |

### 4.4 ChannelAccount Fields

```
channel_id: [u8; 32]           — Channel unique identifier
user_pubkey: Pubkey            — User public key (Party A)
provider_pubkey: Pubkey        — Provider public key (Party B)
token_mint: Pubkey             — SPL Token Mint
status: ChannelStatus          — Open / Challenged / Settling / Closed
sequence: u64                  — Current sequence number
current_root: [u8; 32]         — Current Merkle root
total_deposited: u64           — Total deposited
total_claimed: u64             — Total claimed
vault_a / vault_b: Pubkey      — Token accounts for both parties
deposit_a / deposit_b: u64     — Deposits from each party
challenge_duration: u64        — Dispute period (slots)
min_challenge_delay: u64       — Minimum dispute delay
challenge_slot: Option<u64>    — Dispute initiation slot (set in Challenged state)
settle_deadline: Option<u64>   — Settlement window deadline (set in Settling state)
tree_depth: u32                — Merkle tree depth (max 12)
claimed_leaves: Vec<u32>       — Claimed leaf indices
auto_close_slot: Option<u64>   — Auto-close slot
```

### 4.5 Build and Deploy

```bash
cd ignite-pay-program

# Build
anchor build

# Deploy to Devnet
anchor deploy --provider.cluster devnet

# Deploy to Mainnet
anchor deploy --provider.cluster mainnet
```

### 4.6 Account Space Calculation

```rust
ChannelAccount::space(tree_depth)
// tree_depth=3 → 8 leaves → ~520 bytes
// tree_depth=4 → 16 leaves → ~552 bytes
// tree_depth=8 → 256 leaves → ~1112 bytes
// tree_depth=12 → 4096 leaves → ~16472 bytes
```

---

## 5. Implementation Phases

### Phase 1: Local Development and Testing

**Goal**: Complete the full channel workflow using the off-chain library, without on-chain operations.

```bash
# Build off-chain library
cd ignite-pay-state-channel
cargo test

# Verify all modules
cargo test -- --nocapture
```

Key test scenarios:
1. Open channel → split tree → transfer → cooperative close
2. HTLC creation → reveal preimage → resolve
3. HTLC creation → timeout → refund
4. Pipeline batch operations + abort rollback
5. Compliance limit trigger

### Phase 2: On-chain Devnet Integration

**Goal**: Deploy the on-chain program, implement off-chain operations + on-chain settlement.

#### Step 1: Deploy On-chain Program

```bash
cd ignite-pay-program

# Build
anchor build

# Deploy to Devnet
anchor deploy --provider.cluster devnet

# Record Program ID
# Current: DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe
```

#### Step 2: SVM Integration Testing

```bash
# Run litesvm tests in the Anchor workspace
cd anchor-workspace/tests/svm-litesvm
cargo test
```

#### Step 3: End-to-End Flow Testing

1. User calls `open_channel` → PDA created on-chain
2. Funds transferred to Escrow Vault
3. Off-chain tree split + sign LeafUpdate
4. Provider co-signs
5. Off-chain payment flow
6. Call `cooperative_settle` → enter settlement window
7. Both parties call `claim` to claim leaves
8. Call `finalize_settlement` to close channel

### Phase 3: Hub Routing Network

**Goal**: Multiple Hubs form a routing network, enabling cross-channel payments.

#### Step 1: Hub Registration

```rust
// Each Hub generates DID + key pair
// Register with HubManager
// Report metrics
```

#### Step 2: Route Discovery Testing

```bash
cd ignite-pay-state-channel
cargo test routing
cargo test multihop
```

#### Step 3: Multi-hop Payment Testing

1. Discover route User → Hub1 → Hub2 → Merchant
2. Create HTLCs with decreasing timelock
3. Terminal reveals preimage
4. Reverse hop-by-hop resolution

### Phase 4: Compliance and Audit

**Goal**: Integrate compliance management to support regulatory audits.

#### Step 1: Configure Spending Limits

```rust
compliance.init_channel_compliance(channel_id, SpendingLimit {
    threshold: 1_000_000,
    per_channel: 5_000_000,
    window_slots: 432_000,   // ~1 epoch
})?;
```

#### Step 2: Audit Trail

```rust
// Record each LeafUpdate to audit log
compliance.record_audit(&update)?;
```

---

## 6. Testing Guide

### 6.1 Off-chain Library Unit Tests

```bash
cd ignite-pay-state-channel

# All tests
cargo test

# By module
cargo test --lib channel
cargo test --lib merkle
cargo test --lib signing
cargo test --lib pipeline
cargo test --lib htlc
cargo test --lib hub
cargo test --lib routing
cargo test --lib multihop
cargo test --lib compliance
```

### 6.2 SVM Integration Tests

```bash
# In the Anchor workspace
cd anchor-workspace/tests/svm-litesvm
cargo test
```

### 6.3 Key Test Scenarios

| Scenario | Modules Involved | Verification Points |
|:---------|:-----------------|:--------------------|
| Open → split → transfer → close | channel, pipeline | Amount conservation, signature verification |
| HTLC full lifecycle | htlc, pipeline | Preimage verification, timelock |
| Dispute flow | channel | challenge → counter → settle → claim |
| Multi-hop payment | multihop, routing | Decreasing timelock, fee calculation |
| Compliance trigger | compliance | Sliding window, threshold trigger |
| Pipeline rollback | pipeline | abort/drop automatic recovery |
| Batch update | channel | BatchFailureInfo correct reporting |

---

## 7. Signing Mechanism

### 7.1 Two-layer Signing

**Leaf-level signing** (LeafUpdate) — verified off-chain:
```
message = SHA-256(channel_id || sequence || leaf_index || prev_leaf_hash || new_leaf_hash)
signature = Ed25519.sign(message, signer_private_key)
```

**State-level signing** (SignedState) — pre-hashed off-chain, verified on-chain:
```
Off-chain: message = SHA-256(channel_id || sequence || root)
sig_a = Ed25519.sign(message, user_private_key)
sig_b = Ed25519.sign(message, provider_private_key)
```

### 7.2 On-chain Signing Message Format

The on-chain contract constructs raw byte concatenation (not hashed), verified through Solana ed25519 instruction introspection. There are three families by format:

**Family A — OpenChannel** (user single signature):
```
message = channel_id || deposit_a(8 LE) || tree_depth(4 LE) || initial_root
```

**Family B — CooperativeSettle / SubmitCounterState** (dual party dual signature):
```
message = channel_id || sequence(8 LE) || root
```

**Family C — Claim / VerifyHTLC / HTLCRefund / HTLCRefund / FinalizeSettlement / TriggerChallenge** (single signature):
```
message = channel_id || current_slot(8 LE) || current_root
```

> **Note**: The off-chain `claim_message()` helper function uses the `SHA-256("claim" || channel_id || leaf_index || amount || current_slot)` format, which differs from the on-chain Family C format. On-chain Claim verification uses `channel_id || current_slot || current_root`, which does not include leaf-level fields.

---

## 8. Channel Lifecycle

```
                          ┌───────────┐
                          │   Open    │ ← open_channel (on-chain)
                          └─────┬─────┘
                                │
                    Off-chain operations (transfer, HTLC, split)
                          ┌─────┴─────┐
                          │           │
                 ┌────────▼──┐  ┌─────▼──────────┐
                 │Cooperative│  │    Challenge    │
                 │  Settle   │  │                 │
                 └────┬──────┘  └────┬────────────┘
                      │              │
                      │         ┌────▼────────────┐
                      │         │ Counter State    │
                      │         │ (optional)       │
                      │         └────┬────────────┘
                      │              │
                 ┌────▼──────────────▼────┐
                 │       Settling          │
                 │  (settlement window: claim leaves)  │
                 └────────────┬───────────┘
                              │
                 ┌────────────▼───────────┐
                 │  Finalize Settlement   │
                 │  (distribute unclaimed funds) │
                 └────────────┬───────────┘
                              │
                 ┌────────────▼───────────┐
                 │        Closed          │
                 └────────────────────────┘
```

---

## 9. Troubleshooting

### 9.1 Signature Verification Failure

**Symptom**: `verify_leaf_update_signature` or on-chain `InvalidSignature`

**Investigation**:
1. Check that the signer's public key is correct (User or Provider)
2. Check that `prev_leaf_hash` matches the current leaf
3. Check that the sequence is contiguous
4. Confirm the same `channel_id` is used

### 9.2 Amount Conservation Error

**Symptom**: `AmountConservation { expected, actual }`

**Investigation**:
1. When splitting the tree, ensure the sum of all leaf amounts = `total_deposited`
2. In Pipeline operations, the partial_transfer amount must not exceed the source leaf
3. Check for concurrent modifications

### 9.3 Merkle Proof Verification Failure

**Symptom**: On-chain `ProofVerificationFailed`

**Investigation**:
1. Confirm the off-chain `MerkleTree` uses sorted-pair hashing: `hashv(&[min, max])`
2. Check that the leaf is at the correct index position
3. Confirm `current_root` is up to date

### 9.4 HTLC Timeout/Refund Issues

**Symptom**: `HtlcNotExpired` or `HtlcExpired`

**Investigation**:
1. Solana slot timing: 1 slot ≈ 400ms (normal), devnet may be slower
2. Check that `timelock_slot` satisfies the constraint: `> current_slot + challenge_duration + HTLC_SAFETY_MARGIN`
3. For multi-hop, check that timelock decrement is correct

### 9.5 sled Database Issues

**Symptom**: Database corruption or lock conflicts

**Investigation**:
1. sled does not support multiple processes opening the same database simultaneously
2. Ensure the `sled::open` path has correct filesystem permissions
3. After an abnormal exit, you may need to delete `*.lock` files

---

## 10. Version Upgrade Path

```
V0.1 (current)                      V1.0                         V2.0
┌──────────────────┐    ┌────────────────────────┐    ┌───────────────────────┐
│ Off-chain channel │    │ On-chain program deploy │    │ Hub routing network    │
│ management        │    │ Devnet integration test  │    │ Multi-hop payments     │
│ Single-channel    │ →  │ Cooperative close +      │ →  │ Liquidity management   │
│ payments          │    │ dispute                  │    │ Full compliance engine  │
│ Mock settlement   │    │ HTLC on-chain verify     │    │                       │
│ Pipeline batch    │    │                          │    │                       │
│ operations        │    │                          │    │                       │
└──────────────────┘    └────────────────────────┘    └───────────────────────┘
```

Upgrade highlights:
- **V0.1 → V1.0**: Deploy `ignite-pay-program`, initialize SPL Token accounts, configure on-chain parameters
- **V1.0 → V2.0**: Deploy multiple Hub nodes, configure routing topology, enable compliance module
