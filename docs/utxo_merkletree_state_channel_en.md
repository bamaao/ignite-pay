This design guide aims to provide the development team with a standardized architecture for building an **Off-chain UTXO + Merkle Tree** state channel on top of the **Solana Account Model**. The core objective is to address **concurrency, low latency, and compliance** challenges in high-frequency AI streaming payments.

---

# AI Streaming Payment State Channel Design & Development Guide (UTXO + Merkle Tree Approach)

## 1. Core Design Philosophy

A traditional account-model channel has only a single balance field. Each payment updates the same balance, and the next transaction must wait for the previous one's signature to complete before it can sign based on the new state — this is "head-of-line blocking."

The solution in this design is **Single Merkle Root + UTXO Pre-allocation**:

```
┌─────────────────────────────────────────────────────────────┐
│  Solana On-chain (Channel Account)                           │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  current_root: Root_init (initially all funds to user)    │ │
│  │  sequence: 0  (off-chain negotiated state version starts at 1) │ │
│  │  deposit_a / deposit_b / status / challenge_slot ...     │ │
│  └─────────────────────────────────────────────────────────┘ │
│                          ↕ Submit Root + Proof at settlement  │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  Off-chain Merkle Tree (built after off-chain negotiation,│ │
│  │  both parties hold complete copies)                       │ │
│  │                                                          │ │
│  │  [UTXO_0] [UTXO_1] [UTXO_2] ... [UTXO_N] [Rest]        │ │
│  │   $0.10    $0.10    $0.10       $0.10    $remaining      │ │
│  │  owner:   owner:   owner:      owner:   owner:           │ │
│  │  user     user     user        user     user             │ │
│  │                                                          │ │
│  │  Root_1 (seq=1, signed by both parties) → payments start │ │
│  │  from this state                                          │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

* **Solana as Settlement Layer**: The chain stores only Channel metadata and the Merkle Root, reducing on-chain costs.
* **On-chain First, Then Off-chain**: The user first unilaterally opens a channel on-chain and locks funds (the initial Root is a single-leaf tree). Then off-chain, the user negotiates with the service provider to construct a Merkle Tree with UTXO allocations, and both parties sign to confirm.
* **Off-chain UTXO Pre-allocation**: During off-chain negotiation, the deposited SPL Tokens (stablecoins) are split into N equal-value UTXO leaves + 1 change leaf. Each UTXO is an indivisible "coin."
* **Single Merkle Root Model**: At any given time, there is only one valid Root. No parallel forks exist.
* **Pipelined Parallelism**: UTXO pre-allocation enables the client to **batch-sign** — signing N leaf updates at once, and the service provider replays them in order. Parallelism occurs during the signature preparation phase, not during Root generation.

---

## 2. Key Module Implementation Guide

### A. Off-chain Data Structures (The "Off-chain Ledger")

Each channel maintains a Merkle Tree locally, with both parties holding complete copies.

**Leaf Node Definition**:

```rust
/// Leaf Node — serialized to fixed-length bytes, then SHA-256 hashed as Merkle Hash
struct UTXOLeaf {
    /// Leaf type
    type: LeafType,              // enum { Standard, HTLC, Compliance }

    /// Current holder's public key
    owner: Pubkey,

    /// Amount (smallest token unit, e.g., micro-USDC for USDC)
    amount: u64,

    /// HTLC conditions (only valid when type == HTLC)
    hash_lock: Option<[u8; 32]>,  // SHA-256(preimage R)
    timelock_slot: Option<u64>,   // Solana slot absolute height

    /// HTLC beneficiary (only valid when type == HTLC)
    /// The party providing the correct preimage R can claim this UTXO's funds
    /// After timeout, funds return to owner
    beneficiary: Option<Pubkey>,
}
```

> **Design Notes**:
>
> **Anti-replay**: Guaranteed by the global `sequence` (strictly increasing in LeafUpdate) + `prev_leaf_hash`. Each LeafUpdate must specify `prev_leaf_hash`; the service provider compares it against the local tree to ensure the modification is based on the latest state. Therefore, no additional `leaf_nonce` field is needed.
>
> **Necessity of the `beneficiary` field**: An HTLC leaf has two mutually exclusive settlement paths: (1) the beneficiary providing R claims the funds; (2) after timeout, `owner` reclaims the funds. `owner` remains unchanged during the HTLC lock period (representing the timeout refund recipient), while `beneficiary` specifies who is authorized to claim with R. This allows the on-chain contract to arbitrate clearly: transfer to `beneficiary` when R is verified, transfer to `owner` on timeout.
>
> In a direct channel, `owner = user`, `beneficiary = provider`. In a multi-hop scenario, `owner` = the Hub party for timeout refunds, `beneficiary` = the downstream node (the party providing R).

**Key Design Constraints**:

1. **Amounts are indivisible**: A UTXO can only be transferred as a whole (from owner=A to owner=B), not partially. For precise amounts, appropriate denominations should be pre-allocated when the channel opens (e.g., 10 x $0.01 + 5 x $0.05 + 2 x $0.10).
2. **Fixed leaf count**: The total number of leaves remains constant during the channel's lifetime; only leaf contents (owner/type/condition) are modified.
3. **Change leaf (Rest)**: The last leaf holds the remaining funds after deducting all pre-allocated UTXOs, used for large payments or change when closing the channel.

**State Root**: The hash aggregation value of all UTXO leaves. The on-chain Channel Account stores `current_root`, which serves as the sole valid credential at settlement.

**Empty Leaf Handling**:

When `leaf_count < 2^tree_depth`, there are unallocated empty slots in the tree. An empty leaf is a standard UTXOLeaf struct instance:

```rust
/// Empty leaf constant — initial value for all unused leaf slots
const EMPTY_LEAF: UTXOLeaf = UTXOLeaf {
    type_: LeafType::Standard,
    owner: Pubkey::default(),   // System public key (all zeros)
    amount: 0,
    hash_lock: None,
    timelock_slot: None,
    beneficiary: None,
};

/// Empty leaf hash = SHA-256(borsh_serialize(EMPTY_LEAF))
/// Since all fields are fixed values, this hash is a global constant
const EMPTY_LEAF_HASH: [u8; 32] = sha256(borsh::serialize(&EMPTY_LEAF));
```

Empty leaves use the exact same serialization + hashing process as regular leaves, ensuring consistency in hash computation. At settlement, leaves with `owner == Pubkey::default()` or `amount == 0` are automatically skipped and do not participate in fund allocation.

### B. Pipelined Signing Mechanism

#### Core Principle: Why UTXO Pre-allocation Eliminates Head-of-Line Blocking

Traditional account model:
```
Balance $10 → Sign "pay $0.01" → Balance $9.99 (must wait for signature to complete before signing next)
              blocked ↑
```

UTXO pre-allocation model:
```
Pre-allocated at channel opening:
  UTXO_0($0.01) UTXO_1($0.01) UTXO_2($0.01) ... UTXO_99($0.01) Rest($remaining)

Payment 1: Modify UTXO_0.owner = provider → Compute new Root_2 (based on Root_1)
Payment 2: Modify UTXO_1.owner = provider → Compute new Root_3 (based on Root_2)
Payment 3: Modify UTXO_2.owner = provider → Compute new Root_4 (based on Root_3)
```

**Roots are strictly linear**: Root_1 → Root_2 → Root_3 → ..., no forking. (Root_init is the on-chain initial state seq=0, Root_1 is the off-chain negotiated state seq=1, payments start from Root_2)

**What "no waiting" really means**:

Parallelism is not at the Root level, but at the signature preparation level. The client can sign all payments at once:

```
Client offline batch signing:
  1. Based on Root_1, modify UTXO_0 → get Root_2, sign Sig_2
  2. Based on Root_2, modify UTXO_1 → get Root_3, sign Sig_3
  3. Based on Root_3, modify UTXO_2 → get Root_4, sign Sig_4
  ...
  N. Send all (Root_i, Sig_i) to the service provider at once

Service provider receives and verifies them in order.
```

Because each UTXO is an independent "coin," when the client modifies UTXO_0, it doesn't need to care about UTXO_1's state (they are not on the same leaf path). This allows batch signature computation to be completed quickly in a pipelined fashion.

#### Pipeline Correctness Proof: Why Settlement is Correct Without Waiting for Responses

**Core Question**: The client signs N LeafUpdates consecutively and sends them to the service provider all at once, without waiting for individual confirmations. How can we guarantee that each transaction settles correctly?

**Theorem: Pipeline Atomicity** — If the client signs a LeafUpdate sequence `[LU_2, LU_3, ..., LU_N]` with strictly increasing sequence numbers (starting from seq=2, where seq=1 is the off-chain negotiated state), and the service provider verifies them in order, then the following properties hold:

**Property 1: All-or-Nothing (Cannot Partially Roll Back)**

```
Premise:
  LU_i's new_leaf is the input for LU_{i+1}'s prev_leaf_hash
  (when LU_i and LU_{i+1} modify the same leaf)

Conclusion:
  The service provider can only accept all N updates, or reject all updates from LU_k onward
  It is impossible to have "accepted LU_2 but not LU_3" and then continue accepting LU_4

Proof:
  After verifying LU_k, the service provider updates local_tree, local_sequence = k
  If LU_{k+1} is rejected:
    - If due to prev_leaf_hash mismatch: LU_{k+1}'s dependent state is inconsistent with current tree
      → All LU_{k+2}~LU_N have sequence > k+1, but local_sequence stays at k
      → All subsequent updates are rejected (sequence > local_sequence + 1 check fails)
    - If due to invalid signature: only this one update fails
      → LU_{k+2}'s sequence = k+2 > local_sequence+1 = k+1
      → All subsequent updates are also rejected

  Therefore, once an update fails, all subsequent updates fail. The already-accepted LU_1~LU_k constitute a valid state.
```

**Property 2: Independence of Different Leaves (Independent Commit)**

```
Premise:
  LU_i modifies leaf_a, LU_j modifies leaf_b, and leaf_a ≠ leaf_b

Conclusion:
  LU_i and LU_j have no dependency in the Merkle Tree and can be verified independently

Proof:
  In the Merkle Tree, leaf_a and leaf_b only converge at their lowest common ancestor
  Modifying leaf_a changes Root_a→Root_a', but leaf_b's hash remains unchanged
  Modifying leaf_b changes Root_a'→Root_a'', but this is a new Root

  Key: LU_j's prev_leaf_hash is leaf_b's value under Root_{j-1}
  Since leaf_a ≠ leaf_b, leaf_b is identical under Root_{i-1} and Root_i
  Therefore LU_j.prev_leaf_hash is always correct, regardless of whether LU_i has been processed by the service provider
```

**Property 3: Deterministic Local Computation on Client Side (Deterministic Local State)**

```
When the client signs the pipeline, local state evolution is deterministic:

  State_1 (Root_1, initial state after off-chain negotiation)
    ├── LU_2: leaf_0 changes → State_2 (Root_2)
    ├── LU_3: leaf_1 changes → State_3 (Root_3)
    ├── LU_4: leaf_2 → HTLC → State_4 (Root_4)
    └── LU_5: leaf_3 changes → State_5 (Root_5)

  The input for each step (current tree state) is entirely determined by the previous step
  No external input, no randomness
  → The client's computed Root_2~Root_5 sequence is uniquely determined

  After the service provider verifies in order, its local tree evolution is identical to the client's
  → Both parties hold the same Root at the same sequence
```

**Pipeline Failure Recovery Strategy**:

```
Scenario: Client sends [LU_2, LU_3, LU_4, LU_5], service provider accepts LU_2~LU_3 but rejects LU_4

Root cause analysis:
  a) LU_4.prev_leaf_hash mismatch
     → Client and service provider's tree states diverged at sequence=3
     → Should not happen (Property 3 guarantees determinism), unless:
       - Service provider received an update from someone else before LU_3 (impossible in a two-party channel)
       - Client signed based on incorrect state

  b) LU_4.signature invalid
     → Client signature computation error, re-signing is sufficient

Recovery:
  1. Service provider returns error: { failed_at: 4, reason: "prev_hash_mismatch" | "invalid_sig" }
  2. Client re-signs LU_4', LU_5' from the state at sequence=3
  3. If prev_hash_mismatch: Client requests the service provider's current tree state, syncs, then re-signs

Safety guarantee:
  Already-accepted LU_2, LU_3 will not be rolled back
  Client and service provider's state at sequence ≥ 3 is always consistent
```

#### Protocol Message Format

```rust
/// Leaf Update Instruction — the smallest signing unit sent from client to service provider
struct LeafUpdate {
    /// Channel ID
    channel_id: [u8; 32],

    /// Global state version number this update will produce (strictly increasing)
    sequence: u64,

    /// Index of the leaf being modified
    leaf_index: u32,

    /// Hash of the leaf before modification (service provider can verify consistency with this)
    prev_leaf_hash: [u8; 32],

    /// Leaf plaintext after modification
    new_leaf: UTXOLeaf,

    /// Payer's signature for this update
    /// Signed content = SHA-256(channel_id || sequence || leaf_index || prev_leaf_hash || new_leaf_hash)
    signature: Signature,
}
```

#### Service Provider Processing Flow

```
Service provider receives [LeafUpdate_1, LeafUpdate_2, ...] then:

1. Sort by sequence
2. Verify each one:
   a. sequence == local_sequence + 1  (strictly increasing)
   b. prev_leaf_hash == local_tree.get_leaf(leaf_index).hash()  (based on known state)
   c. signature verification passes
3. After passing, update local Merkle Tree, local_sequence += 1
4. (Optional) Counter-sign confirmation to client
```

**Note**: During verification, the service provider **does not need a Merkle Proof**. The service provider holds a complete tree copy and can directly compare `prev_leaf_hash` with the local leaf hash. Merkle Proofs are only needed during **on-chain settlement**, generated by the service provider from the local tree.

#### HTLC Leaf Pipeline

If `UTXO_5` is of HTLC type and is waiting for preimage R, it does not affect other leaves:

```
UTXO_0 ~ UTXO_4:  Normal pipeline signing, instant payment
UTXO_5:            HTLC state, waiting for service provider to submit R or timeout
UTXO_6 ~ UTXO_99: Continue normal pipeline signing

UTXO_5 does not block any other leaf's signing pipeline.
```

### C. Hash Time-Locked Contract (HTLC) Integration

In per-token billing scenarios, HTLC ensures the service provider must provide a "model output credential" (preimage R) to ultimately receive funds.

#### HTLC Lifecycle

```
Phase 1: Lock
┌──────────────────────────────────────────────────────┐
│  Service provider generates random number R, computes H = SHA-256(R)              │
│  Service provider → User: Send H + service description                             │
│  User signs LeafUpdate:                                                             │
│    UTXO_i.type = HTLC                                                               │
│    UTXO_i.hash_lock = H                                                              │
│    UTXO_i.timelock_slot = current_slot + TIMEOUT                                    │
│    UTXO_i.owner = User           (timeout refund recipient)                         │
│    UTXO_i.beneficiary = Service provider    (beneficiary after providing R)         │
│  New Root contains this HTLC leaf                                                   │
└──────────────────────────────────────────────────────┘

Phase 2: Unlock — Two mutually exclusive paths
┌──────────────────────────────────────────────────────┐
│ Path A: Service provider provides preimage (normal completion)                      │
│   Service provider → User: AI output + R                                            │
│   User verifies H == SHA-256(R), then signs LeafUpdate:                            │
│     UTXO_i.type = Standard                                                          │
│     UTXO_i.owner = Service provider (beneficiary → owner)                           │
│     UTXO_i.hash_lock = None                                                         │
│     UTXO_i.beneficiary = None                                                       │
│                                                                                     │
│   [Key] If user refuses to sign:                                                    │
│   Service provider holds R and can directly submit VerifyHTLC(R + Merkle Proof)     │
│   to the on-chain contract during the challenge period.                             │
│   After the contract verifies SHA256(R)==hash_lock,                                 │
│   it transfers funds to beneficiary (= service provider).                           │
│   Service provider does not depend on user being online.                            │
├──────────────────────────────────────────────────────┤
│ Path B: Timeout refund (service provider did not provide R)                         │
│   After Solana slot > timelock_slot                                                 │
│   User signs LeafUpdate:                                                            │
│     UTXO_i.type = Standard                                                          │
│     UTXO_i.owner = User (funds return to owner)                                    │
│     UTXO_i.hash_lock = None                                                         │
│     UTXO_i.beneficiary = None                                                       │
│                                                                                     │
│   [Key] If HTLC has expired and no R was submitted at on-chain settlement:         │
│   Contract rejects beneficiary's claim, funds go to owner (user).                   │
└──────────────────────────────────────────────────────┘
```

#### Timing Constraints

```
timelock_slot must satisfy:
    timelock_slot > current_slot + CHALLENGE_DURATION + SAFETY_MARGIN

Reason: The challenge period must be long enough for the service provider to submit R on-chain.
If the challenge period is shorter than the HTLC timeout, a malicious user could
use a challenge settlement to "skip" the HTLC before the service provider submits R,
stealing the funds.
```

---

## 3. Business Flows

This chapter describes the complete lifecycle of the state channel from creation to closure, covering four core business flows. All flows are illustrated with concrete examples.

### 3.1 Open State Channel

#### 3.1.1 Flow Overview

> The diagram below is a simplified overview. For detailed steps and parameter descriptions, see 3.1.3.

```
User                                    Service Provider              Solana On-chain
 │                                        │                              │
 │  1. Negotiate channel parameters       │                              │
 │ ──────────────────────────────────────>│                              │
 │  (deposit_amount, denominations,        │                              │
 │   challenge_duration, token_mint)       │                              │
 │                                        │                              │
 │  2. OpenChannel on-chain transaction    │                              │
 │  (User unilaterally deposits, initializes Root_init)                    │
 │ ──────────────────────────────────────────────────────────────────────>│
 │                                        │                     Create ChannelAccount
 │                                        │                     Deposit SPL Token
 │                                        │                     current_root = Root_init
 │                                        │                              │
 │  3. Off-chain negotiation to build Merkle Tree                          │
 │  (User constructs UTXO leaves → sends to service provider              │
 │   → Service provider verifies → Both parties sign Root_1)              │
 │  <──────────────────────────────────────>                              │
 │                                        │                              │
 │  ========== Channel ready, begin off-chain payments ==========         │
```

#### 3.1.2 UTXO Denomination Strategy

When opening a channel, the user selects a UTXO denomination combination based on the expected payment pattern. The denomination choice directly affects parallelism and change frequency.

**Strategy A: Uniform Split (suitable for fixed unit price scenarios)**

```
Scenario: AI API costs $0.01 per call, user deposits $10.00 USDC

Channel parameters:
  leaf_count = 101    // 100 payment leaves + 1 change leaf
  tree_depth = 8      // Maximum 4096 leaves (on-chain program limits tree_depth ≤ 12)

Initial leaves:
  UTXO_0 ~ UTXO_99:  each $0.01, owner=user, type=Standard
  UTXO_100 (Rest):   $9.00, owner=user, type=Standard

  Total: 100 × $0.01 + $9.00 = $10.00 ✓
```

**Strategy B: Multi-denomination Mix (suitable for variable price scenarios)**

```
Scenario: AI API call prices range from $0.01 ~ $1.00, user deposits $10.00 USDC

Initial leaves:
  UTXO_0 ~ UTXO_49:  each $0.01, owner=user    // 50 × $0.01 = $0.50
  UTXO_50 ~ UTXO_59: each $0.05, owner=user    // 10 × $0.05 = $0.50
  UTXO_60 ~ UTXO_69: each $0.10, owner=user    // 10 × $0.10 = $1.00
  UTXO_70 ~ UTXO_74: each $0.50, owner=user    //  5 × $0.50 = $2.50
  UTXO_75:           $5.50, owner=user (Rest)   //  Change = $5.50

  Total: $0.50 + $0.50 + $1.00 + $2.50 + $5.50 = $10.00 ✓
```

**Strategy C: Change-priority (suitable for large infrequent payment scenarios)**

```
Scenario: Expected few large payments, user deposits $100.00 USDC

Initial leaves:
  UTXO_0 ~ UTXO_9:   each $0.10, owner=user    // 10 × $0.10 = $1.00
  UTXO_10:           $99.00, owner=user (Rest)  // Almost everything in change

At payment time: Split exact amount from the Rest leaf (see Section 3.2 split flow)
```

#### 3.1.3 Complete OpenChannel Flow: On-chain Opening → Off-chain Negotiation to Build Tree

**Core Principle**: The user first unilaterally opens the channel on-chain and locks funds, then negotiates with the service provider off-chain to construct the Merkle Tree. The on-chain transaction only anchors an initial Root (all funds to the user), and after off-chain negotiation, both parties sign to produce the first valid split Root.

```
Phase 1: On-chain Opening (User unilateral operation)

  User                                       Solana On-chain
    │                                            │
    │  1. Submit OpenChannel transaction          │
    │  { user_pubkey: user_pubkey,                │
    │    provider_pubkey: provider_pubkey,         │
    │    token_mint: USDC,                         │
    │    deposit_amount: 10_000_000,               │
    │    root_init: [u8; 32],    ← Initial Root   │
    │    tree_depth: 8,                            │
    │    leaf_count: 1,           ← Initially only 1 leaf │
    │    challenge_duration: 86400,                │
    │    min_challenge_delay: 7200,                │
    │    sig_a: sig_user(Root_init) }              │
    │ ──────────────────────────────────────────>│
    │                                            │
    │                              Contract execution: │
    │                              a. Create ChannelAccount │
    │                              b. Verify sig_a (only user signature needed) │
    │                              c. current_root = Root_init │
    │                              d. sequence = 0, status = Open │
    │                              e. Transfer 10 USDC to vault_a │
    │                              f. Record open_slot │
    │                                            │
    │  <── Transaction confirmed, channel_id ─────│
    │                                            │
    │  Root_init contents (constructed by user locally): │
    │    Only 1 leaf:                             │
    │    UTXO_0: { owner=user, amount=10 USDC,    │
    │             type=Standard }                  │
    │    → All funds to user, no service provider pre-approval needed │

Phase 2: Off-chain Negotiation to Build Merkle Tree (Two-party interaction)

  User                                        Service Provider
    │                                            │
    │  2. Send channel creation request + denomination scheme │
    │  { channel_id, deposit: 10 USDC,            │
    │    denominations: [100×$0.01 + Rest],        │
    │    tree_config: {depth:8, count:101} }       │
    │ ──────────────────────────────────────────>│
    │                                            │
    │  3. Service provider confirms and returns its public key │
    │  <──────────────────────────────────────────│
    │  { provider_pubkey: <pubkey>, accepted: true }   │
    │                                            │
    │  4. User locally constructs split Merkle Tree: │
    │     a. Create 101 UTXOLeafs per denominations │
    │     b. All leaves owner = user               │
    │     c. Build Merkle Tree, compute Root_1     │
    │     d. Save all leaf plaintexts + Merkle Proofs │
    │                                            │
    │  5. User sends all leaf plaintexts + Root_1 to service provider │
    │  { leaves: [UTXOLeaf; 101],                  │
    │    root_1: [u8; 32],                          │
    │    sequence: 1 }                              │
    │ ──────────────────────────────────────────>│
    │                                            │
    │  6. Service provider verifies locally:        │
    │     a. Build Merkle Tree locally with received leaves │
    │     b. Compare computed Root == user's Root_1 │
    │     c. Verify all leaves' owner == user       │
    │     d. Verify total_amount == 10 USDC          │
    │                                            │
    │  7. Verification passed, service provider signs Root_1 │
    │  <──────────────────────────────────────────│
    │  sig_provider(channel_id, seq=1, Root_1)    │
    │                                            │
    │  8. User also signs Root_1                    │
    │  sig_user(channel_id, seq=1, Root_1)        │
    │                                            │
    │  At this point both parties hold:             │
    │    - Complete leaf plaintexts (101)            │
    │    - Complete Merkle Tree copy                 │
    │    - Both-party signatures for Root_1 (sequence=1) │
    │    - On-chain Root_init (sequence=0)           │
    │                                            │
    │  ========== Channel ready, begin off-chain payments ===========
```

**Why On-chain First, Then Off-chain**:
- **User fund safety**: After the on-chain transaction is confirmed, funds are locked. The service provider only participates in off-chain negotiation after seeing the on-chain funds, preventing the service provider from backing out after agreeing to cooperate.
- **No need for service provider to be pre-online**: The user can submit the on-chain transaction first, and the service provider can complete the off-chain negotiation when it comes online later.
- **Minimal on-chain initial Root**: `Root_init` contains only 1 leaf (all funds to the user), with zero-cost construction and verification.
- **No risk for service provider**: The service provider only signs after off-chain verification that `total_amount == on-chain deposit`. If the user tampers with the amount, the service provider refuses to sign.

**Relationship Between On-chain Root_init and Off-chain Root_1**:
- `Root_init` (seq=0): On-chain anchor, 1 leaf, all funds to user. This is the on-chain "truth."
- `Root_1` (seq=1): Off-chain negotiation, 101 leaves, dual-signed confirmation. This is the mutually agreed "working state."
- Payments start from seq=2, based on Root_1.
- If off-chain negotiation fails (service provider doesn't respond), the user can close the channel directly based on `Root_init`, with a full refund.

```rust
/// OpenChannel instruction parameters
struct OpenChannelParams {
    /// Deposit amount (smallest token unit)
    deposit_amount: u64,

    /// Initial Merkle Root (single-leaf tree with all funds to user)
    root_init: [u8; 32],

    /// Merkle Tree depth (e.g., 16, expandable off-chain later)
    tree_depth: u32,

    /// Initial leaf count (1 at opening, expands after off-chain negotiation)
    leaf_count: u32,

    /// Challenge period length (slots)
    challenge_duration: u64,

    /// Minimum challenge delay (slots)
    min_challenge_delay: u64,

    /// Auto-close slot (optional)
    auto_close_slot: Option<u64>,

    /// User's signature on Root_init (only user's unilateral signature needed)
    sig_a: Signature,
}
```

#### 3.1.4 Dual-party Funding (Optional)

If the service provider also needs to deposit funds (e.g., collateral, bidirectional payments), use the `FundChannel` instruction. This also follows "on-chain first, then off-chain":

```
1. User first calls OpenChannel (on-chain deposit deposit_a, Root_init all to user)
2. Service provider calls FundChannel(channel_id, deposit_b):
   a. Transfer deposit_b to vault_b
   b. ChannelAccount.deposit_b = deposit_b
3. Off-chain negotiation: Both parties construct a Tree containing both parties' leaves
   a. Total user leaf amount == deposit_a
   b. Total service provider leaf amount == deposit_b
   c. Both parties sign Root_1 (seq=1)
```

> **V1 Simplification**: Initially support only single-party funding (User → Service Provider), no service provider deposit needed.

---

### 3.2 Off-chain UTXO Split & Merge

Although the leaf count is fixed, UTXO splitting and merging can be achieved off-chain by modifying leaf contents (amount/owner/type). The key is that the **change leaf (Rest)** acts as a "reservoir."

#### 3.2.1 Payment Flow: Standard UTXO Transfer

The simplest operation — transfer an entire UTXO to the service provider:

```
State:
  UTXO_0: $0.01, owner=user
  UTXO_1: $0.01, owner=user
  UTXO_100(Rest): $9.98, owner=user

User pays $0.01 to service provider:
  LeafUpdate {
    sequence: 2,
    leaf_index: 0,
    prev_leaf_hash: hash({owner:user, $0.01, Standard}),
    new_leaf: {owner:provider, $0.01, Standard},
  }

Result:
  UTXO_0: $0.01, owner=provider  ← Paid
  UTXO_1: $0.01, owner=user
  UTXO_100(Rest): $9.98, owner=user
```

#### 3.2.2 Split: Create New Denomination from the Rest Leaf

When pre-allocated small UTXOs are exhausted, or a non-standard amount is needed, split from the Rest leaf:

**Scenario**: Need to pay $0.37, but no UTXO with an exact denomination is available.

```
Before split:
  UTXO_0 ~ UTXO_9:  owner=provider (spent)
  UTXO_10 ~ UTXO_99: owner=user, $0.01 each
  UTXO_100(Rest): $9.00, owner=user

Option A: Combinatorial payment (assemble from existing small-denomination UTXOs)

  Pay with 37 × $0.01 UTXOs:
    LeafUpdate(seq=11, UTXO_10→provider)
    LeafUpdate(seq=12, UTXO_11→provider)
    ...
    LeafUpdate(seq=47, UTXO_46→provider)
    Total 37 LeafUpdates

  Pros: No splitting needed
  Cons: Consumes 37 leaf slots, high signature overhead

Option B: Split from Rest (recommended)

  Step 1: Deduct $0.37 from Rest (must deduct first to maintain fund conservation)

    LeafUpdate(seq=11) {
      leaf_index: 100,                  // Rest leaf
      prev_leaf_hash: hash(UTXO_100 current state),
      new_leaf: {owner:user, $8.63, Standard},  // $9.00 - $0.37
    }

  Step 2: Create $0.37 in a free leaf slot (funds come from the deducted Rest)

    Prerequisite: UTXO_0 (spent, owner=provider, $0.01) can be reused

    LeafUpdate(seq=12) {
      leaf_index: 0,                    // Reuse spent slot
      prev_leaf_hash: hash(UTXO_0 current state),
      new_leaf: {owner:user, $0.37, Standard},  // Amount split from Rest
    }

  Step 3: Pay $0.37

    LeafUpdate(seq=13) {
      leaf_index: 0,
      new_leaf: {owner:provider, $0.37, Standard},
    }

  Result:
    UTXO_0: $0.37, owner=provider  ← Paid
    UTXO_10 ~ UTXO_99: Unchanged
    UTXO_100(Rest): $8.63, owner=user

Fund conservation verification:
  Before split: $0.01×10(provider) + $0.01×90(user) + $9.00(Rest) = $10.00
  After split: $0.37×1(provider) + $0.01×9(provider) + $0.01×90(user) + $8.63(Rest) = $10.00 ✓
  At each sequence, sum of all leaf amounts == deposit_a + deposit_b ✓
```

> **Key Principle**: Splitting is essentially **two atomic LeafUpdates** — you must **deduct from Rest first**, then create a new UTXO in a free slot. This order ensures that the fund conservation invariant is never broken in any intermediate state (at each sequence, the sum of all leaf amounts == total deposit). If the order is reversed, funds would be created out of thin air in the intermediate state, which could be exploited maliciously if an on-chain challenge is triggered at that point.

#### 3.2.3 Merge: Reclaim Spent UTXOs

As payments progress, multiple small UTXOs are transferred to the service provider (owner=provider), occupying leaf slots. When free slots run low, merging is needed for reclamation.

**Scenario**: All 100 $0.01 UTXOs have been spent, but Rest still has $9.00 and payments need to continue.

```
Before merge:
  UTXO_0 ~ UTXO_99: owner=provider, $0.01 each (all spent)
  UTXO_100(Rest): $9.00, owner=user

  Problem: No free leaves available!

Merge operation: Consolidate multiple of the service provider's small UTXOs into one

  LeafUpdate(seq=201) {
    leaf_index: 0,    // Keep the first one as the merge result
    new_leaf: {owner:provider, $1.00, Standard},  // Merged amount
  }

  LeafUpdate(seq=202) {
    leaf_index: 1,    // Clear the second one
    new_leaf: {owner:Pubkey::default(), $0.00, Standard},  // Reclaim to empty leaf
  }

  ... Repeat for UTXO_2 ~ UTXO_99 at seq=203~299 ...

  LeafUpdate(seq=299) {
    leaf_index: 99,
    new_leaf: {owner:Pubkey::default(), $0.00, Standard},
  }

After merge:
  UTXO_0: $1.00, owner=provider        ← Merged service provider balance
  UTXO_1 ~ UTXO_99: Empty leaves (reusable)
  UTXO_100(Rest): $9.00, owner=user

  Free slots restored to 99!
```

> **Security Constraint**: The merge operation changes the service provider's total UTXO amount. The service provider must verify:
> 1. Sum of all merged leaves' amounts before merge == merged leaf's amount after merge
> 2. The service provider only merges leaves it owns (only owner=provider leaves can be merged)
> 3. Cleared leaves' amounts must be 0 (prevents creating funds out of thin air)
>
> **Signing Authority Note**: In a bidirectional channel, the service provider can also initiate LeafUpdates (signed by the service provider, verified by the user). When the service provider merges its own leaves, it signs the LeafUpdate, and the user verifies in order. This is symmetric to the user signing payment LeafUpdates — both parties can sign updates modifying leaves they own. Service provider-initiated LeafUpdates also follow the strict sequence increment and prev_leaf_hash matching rules.

#### 3.2.4 Split/Merge Utility Functions

```rust
/// Split a specified amount from the Rest leaf to a target leaf
fn split_from_rest(
    channel_id: &[u8; 32],
    rest_index: u32,
    target_index: u32,
    amount: u64,
    current_sequence: u64,
    tree: &mut MerkleTree,
    signer: &Keypair,
) -> Result<(LeafUpdate, LeafUpdate)> {
    let rest_leaf = tree.get_leaf(rest_index);
    require!(rest_leaf.amount >= amount, "Insufficient Rest balance");
    require!(rest_leaf.owner == signer.pubkey(), "Not the Rest holder");

    // LeafUpdate 1: Rest deduction
    let new_rest = UTXOLeaf {
        type_: LeafType::Standard,
        owner: rest_leaf.owner,
        amount: rest_leaf.amount - amount,
        hash_lock: None,
        timelock_slot: None,
        beneficiary: None,
    };

    let update_rest = LeafUpdate::sign(
        channel_id, current_sequence, rest_index,
        &tree.get_leaf(rest_index), &new_rest, signer,
    );

    // LeafUpdate 2: Target leaf assignment
    let new_target = UTXOLeaf {
        type_: LeafType::Standard,
        owner: signer.pubkey(),
        amount,
        hash_lock: None,
        timelock_slot: None,
        beneficiary: None,
    };

    let update_target = LeafUpdate::sign(
        channel_id, current_sequence + 1, target_index,
        &tree.get_leaf(target_index), &new_target, signer,
    );

    Ok((update_rest, update_target))
}

/// Merge multiple spent leaves into one
/// Note: signer is the party initiating the merge (when service provider merges its own leaves, signer=provider)
fn merge_spent_leaves(
    channel_id: &[u8; 32],
    source_indices: &[u32],
    target_index: u32,
    current_sequence: u64,
    tree: &mut MerkleTree,
    signer: &Keypair,
) -> Result<Vec<LeafUpdate>> {
    let mut total_amount: u64 = 0;
    let mut updates = Vec::with_capacity(source_indices.len() + 1);
    let mut seq = current_sequence;

    // Accumulate amounts from all source leaves
    for &idx in source_indices {
        let leaf = tree.get_leaf(idx);
        require!(leaf.owner == signer.pubkey(), "Can only merge own leaves");
        total_amount = total_amount.saturating_add(leaf.amount);
    }

    // LeafUpdate: Merge into target leaf
    let merged_leaf = UTXOLeaf {
        type_: LeafType::Standard,
        owner: signer.pubkey(),
        amount: total_amount,
        hash_lock: None,
        timelock_slot: None,
        beneficiary: None,
    };
    updates.push(LeafUpdate::sign(
        channel_id, seq, target_index,
        &tree.get_leaf(target_index), &merged_leaf, signer,
    ));
    seq += 1;

    // LeafUpdate: Clear source leaves
    let empty = UTXOLeaf {
        type_: LeafType::Standard,
        owner: Pubkey::default(),
        amount: 0,
        hash_lock: None,
        timelock_slot: None,
        beneficiary: None,
    };
    for &idx in source_indices {
        if idx == target_index { continue; }
        updates.push(LeafUpdate::sign(
            channel_id, seq, idx,
            &tree.get_leaf(idx), &empty, signer,
        ));
        seq += 1;
    }

    Ok(updates)
}
```

---

### 3.3 Off-chain HTLC Settlement

HTLC is used for "pay-after-service" scenarios: the service provider receives payment only after delivering the AI output. This section describes in detail the complete off-chain interaction of HTLC from creation to settlement.

#### 3.3.1 HTLC Creation: Locking Phase

```
Scenario: User requests AI service, provider quotes $0.05

User                                    Provider
 │                                        │
 │  1. Request service                    │
 │  "Please translate this text for me"   │
 │ ──────────────────────────────────────>│
 │                                        │
 │  2. Provider quotes + hash commitment  │
 │  <──────────────────────────────────────│
 │  { price: $0.05, hash_lock: H,          │
 │    timelock_slot: current+5000,          │
 │    description: "AI Translation" }       │
 │                                        │
 │  3. User locks UTXO                    │
 │  Select UTXO_50 ($0.05) to create HTLC:│
 │                                        │
 │  LeafUpdate(seq=20) {                   │
 │    leaf_index: 50,                      │
 │    new_leaf: {                          │
 │      type: HTLC,                        │
 │      owner: user,                       │
 │      amount: $0.05,                     │
 │      hash_lock: H,                      │
 │      timelock_slot: current_slot+5000,  │
 │      beneficiary: provider,             │
 │    }                                    │
 │  }                                      │
 │ ──────────────────────────────────────>│
 │                                        │
 │                                        │  4. Provider verifies:
 │                                        │     a. seq==local_seq+1 ✓
 │                                        │     b. prev_hash matches ✓
 │                                        │     c. Signature is valid ✓
 │                                        │     d. hash_lock == H ✓
 │                                        │     e. timelock is reasonable ✓
 │                                        │
 │                                        │  5. Provider starts processing request
 │                                        │  (and remembers the value of R)
```

#### 3.3.2 HTLC Normal Settlement: Provider Provides Preimage

```
User                                    Provider
 │                                        │
 │                                        │  6. AI processing complete, result obtained
 │                                        │
 │  7. Return AI result + preimage R       │
 │  <──────────────────────────────────────│
 │  { result: "Translation: Hello World",  │
 │    preimage: R }                         │
 │                                        │
 │  8. User verifies:                      │
 │     SHA-256(R) == H? ✓                  │
 │     Result quality satisfactory? ✓      │
 │                                        │
 │  9. User releases HTLC to provider      │
 │  LeafUpdate(seq=21) {                   │
 │    leaf_index: 50,                      │
 │    new_leaf: {                          │
 │      type: Standard,                    │
 │      owner: provider,        ← Funds transferred to provider
 │      amount: $0.05,                     │
 │      hash_lock: None,                   │
 │      timelock_slot: None,               │
 │      beneficiary: None,               │
 │    }                                    │
 │  }                                      │
 │ ──────────────────────────────────────>│
 │                                        │
 │                                        │  10. Provider verifies and accepts
 │                                        │  $0.05 received!
 │                                        │
 │  ========== HTLC Complete ==========
```

#### 3.3.3 HTLC Dispute Path: User Refuses to Release

```
User                                    Provider                     Solana On-chain
 │                                        │                              │
 │  (User receives R but refuses to sign seq=21)                        │
 │                                        │                              │
 │                                        │  Provider holds R, does not depend on user
 │                                        │                              │
 │                                        │  Option A: Trigger on-chain verification
 │                                        │                              │
 │                                        │  TriggerChallenge            │
 │                                        │  (root, seq=20, sig_provider)│
 │                                        │ ────────────────────────────>│
 │                                        │                              │
 │                                        │  VerifyHTLC                  │
 │                                        │  (leaf=50, proof, R)         │
 │                                        │ ────────────────────────────>│
 │                                        │                     Contract verifies:
 │                                        │                     proof ✓
 │                                        │                     HTLC type ✓
 │                                        │                     SHA256(R)==H ✓
 │                                        │                     slot≤timelock ✓
 │                                        │                     Submitter==beneficiary ✓
 │                                        │                              │
 │                                        │                     $0.05 → beneficiary (provider)
 │                                        │                              │
 │                                        │  ... Settlement after challenge period ends ...
```

#### 3.3.4 HTLC Timeout Path: Provider Fails to Provide R

```
User                                    Provider                     Solana On-chain
 │                                        │                              │
 │  (Provider fails to deliver AI output, does not send R)              │
 │                                        │                              │
 │  Wait until current_slot > timelock_slot                             │
 │                                        │                              │
 │  User reclaims HTLC:                   │                              │
 │  LeafUpdate(seq=21) {                   │                              │
 │    leaf_index: 50,                      │                              │
 │    new_leaf: {                          │                              │
 │      type: Standard,                    │                              │
 │      owner: user,            ← Funds returned │                       │
 │      amount: $0.05,                     │                              │
 │      hash_lock: None,                   │                              │
 │      timelock_slot: None,               │                              │
 │      beneficiary: None,               │                              │
 │    }                                    │                              │
 │  }                                      │                              │
 │ ──────────────────────────────────────>│                              │
 │                                        │  Provider verifies slot > timelock:
 │                                        │  Cannot provide R → Accepts refund
 │                                        │                              │
 │  ========== HTLC Refund Complete ==========
```

If the provider is offline or refuses to accept the refund, the user can force a refund through the on-chain HTLCRefund instruction.

#### 3.3.5 Multiple Concurrent HTLCs

Multiple HTLCs can exist simultaneously, each occupying different UTXO leaves, without blocking each other:

```
Current state:
  UTXO_0: $0.01, owner=provider    (paid)
  UTXO_1: $0.01, owner=provider    (paid)
  UTXO_2: HTLC, $0.05, H_1, owner=user, beneficiary=provider    (awaiting translation result)
  UTXO_3: HTLC, $0.10, H_2, owner=user, beneficiary=provider    (awaiting summary result)
  UTXO_4: HTLC, $0.03, H_3, owner=user, beneficiary=provider    (awaiting code generation)
  UTXO_5: $0.01, owner=user        (available)
  ...
  UTXO_100(Rest): $8.80, owner=user

Concurrent processing:
  - UTXO_2's R_1 arrives → User signs seq=N to release
  - UTXO_4's R_3 arrives → User signs seq=N+1 to release
  - Meanwhile UTXO_5 is still available for direct payment → seq=N+2
  - UTXO_3's R_2 has not arrived yet → Continue waiting, does not block others
```

#### 3.3.6 HTLC Preimage Generation and Management

```rust
/// Provider's tool for managing HTLC preimages
struct HtlcManager {
    /// Active HTLCs: hash_lock → (preimage, created_at, amount)
    active_htlcs: HashMap<[u8; 32], HtlcRecord>,
}

struct HtlcRecord {
    /// Preimage R (kept private by provider until service is complete)
    preimage: [u8; 32],
    /// Creation time
    created_slot: u64,
    /// Amount
    amount: u64,
    /// Associated leaf index
    leaf_index: u32,
    /// State
    state: HtlcState,   // Pending | Fulfilled | Expired
}

impl HtlcManager {
    /// Create an HTLC commitment for a service request
    fn create_htlc(
        &mut self,
        amount: u64,
        leaf_index: u32,
        current_slot: u64,
    ) -> ([u8; 32], [u8; 32]) {
        let preimage: [u8; 32] = rand::random();
        let hash_lock = sha256(&preimage);
        self.active_htlcs.insert(hash_lock, HtlcRecord {
            preimage,
            created_slot: current_slot,
            amount,
            leaf_index,
            state: HtlcState::Pending,
        });
        (hash_lock, preimage)
    }

    /// After service completion, reveal preimage to user
    fn reveal_preimage(&mut self, hash_lock: &[u8; 32]) -> Option<[u8; 32]> {
        self.active_htlcs.get_mut(hash_lock).map(|record| {
            record.state = HtlcState::Fulfilled;
            record.preimage
        })
    }

    /// Clean up expired or completed HTLCs
    fn cleanup(&mut self, current_slot: u64, timelock_default: u64) {
        self.active_htlcs.retain(|_, record| {
            match record.state {
                HtlcState::Pending => current_slot < record.created_slot + timelock_default,
                _ => false, // Remove completed or expired ones
            }
        });
    }
}
```

---

### 3.4 Close Channel

There are three paths for channel closure, chosen based on the level of cooperation between the parties:

```
                        Channel Open
                           │
               ┌───────────┼───────────┐
               │           │           │
         Cooperative     Dispute     Auto-close
         Close           Close       (Auto-close)
               │           │           │
               └───────────┼───────────┘
                           │
                     Channel Closed
```

#### 3.4.1 Cooperative Close

Both parties agree to close, and funds are distributed according to the latest state. This is the most efficient way to close, requiring only one on-chain transaction to open the settlement window.

```
User                                    Provider                     Solana On-chain
 │                                        │                              │
 │  1. User requests close                 │                              │
 │  "CloseRequest(Root_latest, seq=N)"    │                              │
 │ ──────────────────────────────────────>│                              │
 │                                        │                              │
 │                                        │  2. Provider verifies latest state
 │                                        │     Confirms Root_latest matches
 │                                        │                              │
 │  3. Provider signs (Root_latest, N)     │                              │
 │  <──────────────────────────────────────│                              │
 │  sig_provider                           │                              │
 │                                        │                              │
 │  4. User submits CooperativeSettle      │                              │
 │  (Root_latest, N, sig_user, sig_provider)                             │
 │ ──────────────────────────────────────────────────────────────────────>│
 │                                        │                     Verify dual signatures ✓
 │                                        │                     sequence ✓
 │                                        │                     Enter Settling state
 │                                        │                     settle_deadline = slot+CLAIM_WINDOW
 │                                        │                              │
 │  5. Each party submits Claims           │                              │
 │                                        │                              │
 │  User claims own leaves:                │                              │
 │  Claim(Rest: $8.63, proof)              │                              │
 │  Claim(UTXO_10~99: unspent, proof)      │                              │
 │ ──────────────────────────────────────────────────────────────────────>│
 │                                        │                              │
 │  Provider claims own leaves:            │                              │
 │  Claim(UTXO_0~9: paid, proof)           │                              │
 │ ──────────────────────────────────────────────────────────────────────>│
 │                                        │                              │
 │  6. FinalizeSettlement                  │                              │
 │ ──────────────────────────────────────────────────────────────────────>│
 │                                        │                     Unclaimed funds returned proportionally
 │                                        │                     status = Closed
 │ <──────── Funds to respective Token Accounts ────────────────────────│
```

> **Optimization**: During cooperative close, all Claims can be merged into the CooperativeSettle transaction (if the number of leaves is small), completing everything in one step. However, this increases the single transaction size, subject to Solana's 1232-byte transaction limit.

#### 3.4.2 Dispute Close

When the two parties disagree on the latest state, or one party is unresponsive, the channel is closed through a challenge mechanism.

**Scenario A: User Initiates Challenge (Provider Unresponsive to Close Request)**

```
User                                    Provider                     Solana On-chain
 │                                        │                              │
 │  (User requests close, provider is unresponsive)                     │
 │                                        │                              │
 │  1. User unilaterally submits TriggerChallenge                        │
 │  (Root_latest, seq=50, sig_user)        │                              │
 │ ──────────────────────────────────────────────────────────────────────>│
 │                                        │                     Verify sig_user ✓
 │                                        │                     sequence > on_chain ✓
 │                                        │                     slot >= open_slot + delay ✓
 │                                        │                     status = Challenged
 │                                        │                     challenge_slot = current
 │                                        │                              │
 │                                        │  2a. Provider submits a better state
 │                                        │  (if provider has a signature with a higher seq)
 │                                        │  SubmitCounterState           │
 │                                        │  (Root, seq=55, sig_a, sig_b) │
 │                                        │ ────────────────────────────>│
 │                                        │                     seq 55 > 50 ✓
 │                                        │                     Verify dual signatures ✓
 │                                        │                     Update root & sequence
 │                                        │                              │
 │  Or                                     │                              │
 │                                        │                              │
 │                                        │  2b. Provider unresponsive    │
 │                                        │  (No one submits a better state during challenge period)
 │                                        │                              │
 │  3. After challenge period expires      │                              │
 │  SettleAfterTimeout                    │                              │
 │ ──────────────────────────────────────────────────────────────────────>│
 │                                        │                     status = Settling
 │                                        │                              │
 │  4. Claim + FinalizeSettlement          │                              │
 │  (Same claim process as cooperative close)                            │
```

**Scenario B: Provider Initiates Challenge (User Maliciously Refuses to Pay)**

```
User                                    Provider                     Solana On-chain
 │                                        │                              │
 │  (User consumed service but refuses to sign LeafUpdate)              │
 │                                        │                              │
 │                                        │  1. Provider triggers challenge
 │                                        │  TriggerChallenge             │
 │                                        │  (Root, seq=48, sig_provider) │
 │                                        │ ────────────────────────────>│
 │                                        │                     status = Challenged
 │                                        │                              │
 │                                        │  2. Provider simultaneously submits HTLC preimage
 │                                        │  VerifyHTLC(leaf=2, proof, R) │
 │                                        │ ────────────────────────────>│
 │                                        │                     SHA256(R)==H ✓
 │                                        │                     $0.05 → beneficiary (provider)
 │                                        │                              │
 │  3. User can submit a better state (if available)                    │
 │  SubmitCounterState                     │                              │
 │  (Root, seq=50, sig_a, sig_b)           │                              │
 │ ──────────────────────────────────────────────────────────────────────>│
 │                                        │                     Update root & sequence
 │                                        │                              │
 │  ... Challenge period ends → SettleAfterTimeout → Claim → FinalizeSettlement ...
```

#### 3.4.3 Auto-close

The channel automatically triggers settlement after `auto_close_slot` expires, preventing funds from being permanently locked.

```
Anyone (Relayer / Watchtower / User / Provider)
 │
 │  current_slot >= auto_close_slot ?
 │
 │  SettleAfterTimeout
 │  (No challenge period required, directly enters Settling)
 │ ──────────────────────────────────>│
 │                                    status = Settling
 │                                    settle_deadline = slot + CLAIM_WINDOW
 │
 │  ... Claim + FinalizeSettlement ...
```

> **Watchtower Mode**: Users can delegate a Watchtower service to monitor `auto_close_slot`. When the channel expires, the Watchtower automatically triggers settlement without the user needing to be online. The provider can also run their own Watchtower.

#### 3.4.4 Close Path Comparison

| Feature | Cooperative Close | Dispute Close | Auto-close |
|:--------|:------------------|:--------------|:-----------|
| On-chain transaction count | 1 + N Claims + 1 Finalize | 1~3 + N Claims + 1 Finalize | 1 + N Claims + 1 Finalize |
| Time required | ~1 slot + Claim window | Challenge period + Claim window | ~1 slot + Claim window |
| Requires counterparty cooperation | Yes (dual signatures) | No (single signature sufficient to trigger) | No |
| Applicable scenario | Normal business conclusion | Counterparty unresponsive or malicious | Channel expired / both parties offline |
| HTLC handling | Settle all off-chain before closing | On-chain VerifyHTLC / HTLCRefund | Same as dispute close |
| Fund security | Highest (both parties agree) | Depends on latest state submitted | Depends on last submitted state |

#### 3.4.5 HTLC Cleanup Before Closing

Before closing the channel, all HTLC leaves should be cleaned up as much as possible to reduce the complexity of on-chain operations:

```
Pre-close checklist:

1. Iterate through all leaves to find those with type == HTLC
2. For each HTLC:
   a. If provider holds R and user has verified: Sign LeafUpdate to release (off-chain)
   b. If provider has not provided R and it has timed out: Sign LeafUpdate to return (off-chain)
   c. If provider holds R but user refuses: Mark for on-chain VerifyHTLC
   d. If HTLC has not timed out and provider has not provided R: Wait for timeout then return
3. After all HTLCs are cleaned up, execute close
4. For HTLCs that cannot be cleaned up off-chain (cases c/d), handle during on-chain settlement window
```

---

## 4. Solana On-chain Program Logic

Implemented using the Anchor framework. The on-chain program is the **final arbiter** -- it only intervenes during channel closure/disputes.

### 4.1 On-chain Account Structure

```rust
#[account]
pub struct ChannelAccount {
    /// Channel unique identifier
    pub channel_id: [u8; 32],

    /// Payer (user)
    pub user_pubkey: Pubkey,

    /// Payee (provider)
    pub provider_pubkey: Pubkey,

    /// Staked SPL Token Mint (e.g., USDC)
    pub token_mint: Pubkey,

    /// Channel status
    pub status: ChannelStatus,

    /// Current global state version number (strictly increasing, only accepts submissions > this value during challenges)
    pub sequence: u64,

    /// Current Merkle Root (on-chain source of truth)
    pub current_root: [u8; 32],

    /// Cumulative total staked (deposit_a + deposit_b)
    pub total_deposited: u64,

    /// Slot at channel creation (used to calculate min_challenge_delay)
    pub open_slot: u64,

    /// Solana slot when challenge started
    pub challenge_slot: Option<u64>,

    /// User's staking Token Account
    pub vault_a: Pubkey,

    /// Provider's staking Token Account (optional, used for dual-funding)
    pub vault_b: Pubkey,

    /// User's initial deposit amount (recorded at channel opening, used to return unclaimed funds during settlement)
    pub deposit_a: u64,

    /// Provider's initial deposit amount (used for dual-funding)
    pub deposit_b: u64,

    /// Challenge period length (Solana slots)
    pub challenge_duration: u64,

    /// Minimum slot interval between Open and when triggering a Challenge is allowed
    /// Prevents malicious parties from immediately triggering a challenge right after channel opening (front-running attack prevention)
    pub min_challenge_delay: u64,

    /// Claimed amount tracking — total amount extracted through Claim/VerifyHTLC/HTLCRefund during the settlement window
    /// Used to determine if settlement is complete
    pub total_claimed: u64,

    /// Settlement window end slot (set when entering Settling state)
    pub settle_deadline: Option<u64>,

    /// Merkle Tree parameters
    pub tree_depth: u32,        // Max 12 (on-chain limit), determines Merkle Proof depth
    pub leaf_count: u32,        // 1 at opening, actual leaf count after off-chain negotiation (e.g., 101)
                                 // Note: This field is not automatically updated after off-chain negotiation,
                                 // it serves as an informational field only. On-chain verification relies solely on current_root,
                                 // not on leaf_count.

    /// Set of claimed leaf indices — prevents the same leaf from being claimed multiple times
    /// Only used in Settling state
    /// Uses Vec<u32> on-chain (Anchor serialization), BTreeSet<u32> off-chain
    pub claimed_leaves: Vec<u32>,

    /// Auto-close slot — after expiry, anyone can trigger settlement
    /// Prevents funds from being permanently locked when both parties are offline
    pub auto_close_slot: Option<u64>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq)]
pub enum ChannelStatus {
    /// Channel operating normally, signing off-chain
    Open,
    /// Challenge period, waiting for both parties to submit latest state
    Challenged,
    /// Settlement in progress
    Settling,
    /// Channel closed, funds distributed
    Closed,
}
```

### 4.2 Instruction List

| Instruction | Triggered By | Description |
|:---|:---|:---|
| `OpenChannel` | User | User unilaterally stakes SPL Token to Vault, records `deposit_a`, initializes `current_root = Root_init` (single-leaf tree with all funds belonging to user), sets `sequence = 0`, records `open_slot`. Only requires user signature, no provider pre-signature needed |
| `FundChannel` | Provider | Provider adds stake to an already-opened channel (dual-funding). Transfers SPL Token to `vault_b`, updates `deposit_b`, `total_deposited`, `leaf_count`. Only provider can call |
| `CooperativeSettle` | Both parties | Both parties submit the latest `(root, sequence, sig_a, sig_b)`, the program verifies both signatures and `sequence > on_chain.sequence`, then sets the channel status to `Settling`, with `settle_deadline = current_slot + CLAIM_WINDOW`. Note: This instruction **does not directly distribute funds**, it only opens the settlement window |
| `TriggerChallenge` | Single party | Submits `(root, sequence, sig)` (own signature). Prerequisite: `current_slot >= open_slot + min_challenge_delay`. The program verifies the signature is valid and `sequence > on_chain.sequence`, sets status to `Challenged`, records `challenge_slot` |
| `SubmitCounterState` | Counterparty | During the challenge period, submits an updated `(root, sequence, sig_a, sig_b)` dual-signed state with `sequence > on_chain.sequence`. The program verifies both signatures then updates Root and Sequence. The dual-signature requirement ensures only mutually agreed states can be submitted as counter-evidence, preventing either party from unilaterally forging a state |
| `SettleAfterTimeout` | Anyone | Triggered after challenge period expires (`current_slot > challenge_slot + challenge_duration`), channel enters `Settling` state, with `settle_deadline = current_slot + CLAIM_WINDOW` |
| `Claim` | Leaf owner or their delegate | **Available during settlement window**: Submits `(leaf_index, leaf_data, merkle_proof)`, program verifies: (1) channel status is `Settling`, (2) `current_slot <= settle_deadline`, (3) proof is valid in `current_root`, (4) `leaf_data` serialized hash matches leaf_hash in proof, (5) `leaf_data.amount > 0` and `leaf_data.owner != Pubkey::default()`. After verification, transfers `leaf_data.amount` from Vault to `leaf_data.owner`'s associated Token Account, increments `total_claimed`. Note: Anyone can submit the Claim transaction (sponsoring gas), but funds can only be transferred to the `owner` recorded in the leaf |
| `VerifyHTLC` | Beneficiary | **Challenged or Settling state**: Beneficiary submits `(leaf_index, merkle_proof, preimage R)`, program verifies: (1) proof is valid in current Root, (2) leaf is HTLC type, (3) SHA-256(R) == hash_lock, (4) current_slot <= timelock_slot, (5) submitter == beneficiary. After verification, adds the leaf amount to `total_claimed`, transfers funds to `beneficiary`, leaf marked as claimed (tracked via off-chain bitmap or independent Claim records) |
| `HTLCRefund` | Owner | **Challenged or Settling state**: HTLC owner submits proof that `current_slot > timelock_slot` for a certain HTLC leaf and it has not been claimed via VerifyHTLC (no R submitted). Program verifies: (1) proof is valid, (2) HTLC has expired, (3) submitter == owner. After verification, transfers `leaf.amount` to `owner`, increments `total_claimed` |
| `FinalizeSettlement` | Anyone | Triggered after `settle_deadline` expires: Returns all unclaimed funds (`deposit_a + deposit_b - total_claimed`) to `vault_a` / `vault_b` in proportion to initial deposits, sets channel status to `Closed` |

### 4.3 Two-Layer Signature System

This scheme uses two different levels of signature schemes for off-chain payment confirmation and on-chain dispute arbitration:

| Signature Level | Signed Content | Purpose | Verifier |
|:----------------|:---------------|:--------|:---------|
| **Leaf Signature** | `SHA-256(channel_id \|\| sequence \|\| leaf_index \|\| prev_leaf_hash \|\| new_leaf_hash)` | Off-chain LeafUpdate payment confirmation | Provider (off-chain verification) |
| **State Signature** | `SHA-256(channel_id \|\| sequence \|\| root)` | On-chain CooperativeSettle / Challenge dispute arbitration | Program (on-chain verification) |

**Leaf Signature**: Signed by the initiator of each leaf modification for every LeafUpdate. In user-payment scenarios, this is typically the user signing (transferring their own UTXO to the provider); in merge/rebalance scenarios, the provider can also sign (merging leaves they own). The provider holds a complete tree replica and can directly verify. This signature ensures that every leaf modification has the authorization of the legitimate initiator.

**State Signature**: Each party signs the complete (root, sequence) pair. This signature is used for on-chain instructions to prove that both parties agree on a global state.

#### Off-chain Signature Message Construction

The off-chain library (`signing.rs`) performs SHA-256 hashing on all messages before signing, returning `[u8; 32]`:

```rust
/// Leaf update message — off-chain LeafUpdate signature
fn leaf_update_message(
    channel_id: &[u8; 32], sequence: u64, leaf_index: u32,
    prev_leaf_hash: &[u8; 32], new_leaf_hash: &[u8; 32],
) -> [u8; 32] {
    // SHA-256(channel_id || sequence || leaf_index || prev_leaf_hash || new_leaf_hash)
    // Preimage length: 32 + 8 + 4 + 32 + 32 = 108 bytes
}

/// State message — off-chain Root signature
fn state_message(channel_id: &[u8; 32], sequence: u64, root: &[u8; 32]) -> [u8; 32] {
    // SHA-256(channel_id || sequence || root)
    // Preimage length: 32 + 8 + 32 = 72 bytes
}

/// Claim message — Claim signature
fn claim_message(
    channel_id: &[u8; 32], leaf_index: u32, amount: u64, current_slot: u64,
) -> [u8; 32] {
    // SHA-256("claim" || channel_id || leaf_index || amount || current_slot)
    // Preimage length: 5 + 32 + 4 + 8 + 8 = 57 bytes
}
```

#### On-chain Signature Message Construction

The on-chain program constructs raw byte concatenation for each instruction (no hashing), passing it directly to Solana's ed25519 instruction introspection for signature verification. Ed25519 internally performs SHA-512 processing during verification. Messages are divided into three families by format:

**Family A — OpenChannel (76 bytes)**:

```
[channel_id: 32] || [deposit_a: 8 LE] || [tree_depth: 4 LE] || [initial_root: 32]
```

- Signer: `channel.user_pubkey` (user single signature)
- The only message format containing `deposit_a`(u64) and `tree_depth`(u32)

**Family B — CooperativeSettle / SubmitCounterState (72 bytes)**:

```
[channel_id: 32] || [sequence: 8 LE] || [root: 32]
```

- Signer: `sig_a` + `sig_b` dual signature (both parties)
- `sequence` and `root` come from instruction parameters (not on-chain stored values)

**Family C — TriggerChallenge / Claim / VerifyHTLC / HTLCRefund / FinalizeSettlement (72 bytes)**:

```
[channel_id: 32] || [current_slot: 8 LE] || [current_root: 32]
```

- Signer: Single signature (submitter/claimant/caller)
- `current_slot` comes from `Clock::slot` (on-chain clock), `current_root` comes from `channel.current_root` (on-chain storage)
- **Note**: This uses `current_slot` instead of `sequence`, which differs from Family B format

**SettleAfterTimeout**: No signature verification.

#### On-chain vs Off-chain Signature Differences

| Comparison | Off-chain (`signing.rs`) | On-chain (`lib.rs`) |
|:-----------|:-------------------------|:--------------------|
| Hashing | Pre-hash SHA-256 → `[u8; 32]` | No hashing, passes raw bytes |
| State message fields | `channel_id \|\| sequence \|\| root` | Same (Family B) |
| Claim message | `"claim" \|\| channel_id \|\| leaf_index \|\| amount \|\| current_slot` | `channel_id \|\| current_slot \|\| current_root` (no leaf-level fields) |

> **Note**: The off-chain `claim_message` and the on-chain `Claim` instruction have different signature message formats. The off-chain version includes a `"claim"` prefix and leaf-level fields, while the on-chain version uses channel-level `current_slot || current_root`. This means off-chain generated Claim signatures cannot be directly used for on-chain Claim instructions; signatures must be reconstructed before submitting the on-chain transaction.

#### Contract Verification Logic

- `OpenChannel`: User single signature, verifies `user_pubkey`
- `CooperativeSettle`: Requires `sig_a` + `sig_b` dual signatures
- `TriggerChallenge`: Only requires submitter's single signature
- `SubmitCounterState`: Requires `sig_a` + `sig_b` dual signatures (ensures the submitted state is the latest mutually agreed state, preventing either party from submitting a forged or outdated single-signed state)
- `Claim` / `VerifyHTLC` / `HTLCRefund`: Claimant single signature
- `FinalizeSettlement`: Caller single signature
- All submissions involving `sequence` must satisfy `sequence > on_chain.sequence` (rollback attack prevention)

#### Provider Co-signing Protocol

CooperativeSettle requires signatures from both parties, so the provider must sign the state root during normal operations. The co-signing protocol is as follows:

```
Provider co-signing flow:

1. User sends a batch of LeafUpdates to the provider
2. Provider verifies each one sequentially by sequence
3. Provider updates local Merkle Tree, obtaining latest Root_latest
4. Provider signs (channel_id, sequence_latest, Root_latest)
5. Provider returns (Root_latest, sequence_latest, sig_provider) to user

User locally stores sig_provider for subsequent CooperativeSettle or as evidence of provider-acknowledged state.

Co-signing frequency strategies:
- Real-time co-signing: Co-sign on every LeafUpdate received (lowest latency, high communication overhead)
- Batch co-signing: Co-sign every N LeafUpdates or every T seconds (recommended, balances latency and overhead)
- On-demand co-signing: User requests provider to co-sign the latest state when needing to close the channel
```

**Security Property**: Once the provider signs a (root, sequence) pair, it acknowledges that state. The provider cannot submit a more recent state on-chain (unless the user later signs LeafUpdates with a higher sequence). This ensures the trustworthiness of CooperativeSettle's dual signatures.

---

## 5. On-chain Settlement Fund Distribution

The on-chain program only stores `current_root` (a 32-byte hash) and cannot directly extract leaf data. Therefore, settlement uses a **claim-based mechanism**: each party actively submits leaf data + Merkle Proof, and the program verifies and distributes funds after validation.

### 5.1 Settlement Trigger Methods

Both methods can trigger settlement with the same result — the channel enters the `Settling` state:

1. **Cooperative Settlement**: Both parties submit `CooperativeSettle(root, seq, sig_a, sig_b)` → Verify dual signatures → Enter Settling
2. **Dispute Settlement**: `TriggerChallenge` → Challenge period → `SettleAfterTimeout` → Enter Settling

### 5.2 Claim Process

```
Settlement window (settle_deadline - current_slot = CLAIM_WINDOW, e.g., 1000 slots):

┌──────────────────────────────────────────────────────────────┐
│  1. Channel enters Settling state                             │
│                                                               │
│  2. Each party submits Claim instructions during the window:  │
│     Claim(leaf_index, leaf_data, merkle_proof)                │
│     → Program verifies proof is valid in current_root          │
│     → Verifies leaf_data serialized hash matches leaf_hash in proof │
│     → Transfers leaf_data.amount from Vault to leaf_data.owner │
│     → total_claimed += leaf_data.amount                        │
│                                                               │
│  3. Special handling:                                          │
│     - VerifyHTLC claimed leaves: Funds transferred to beneficiary, │
│       total_claimed incremented, leaf cannot be Claimed again  │
│       (Program tracks via processed leaves set to prevent duplicate claims) │
│     - HTLC expired without R provided: HTLCRefund transfers to owner │
│       total_claimed incremented, leaf cannot be Claimed or VerifyHTLC'd again │
│     - Empty leaves (owner=Pubkey::default() or amount==0): No Claim needed │
│                                                               │
│  4. Duplicate claim prevention mechanism:                      │
│     Program maintains a processed leaves set (claimed_leaves: Set<u32>): │
│     - Claim succeeds → claimed_leaves.insert(leaf_index)       │
│     - VerifyHTLC succeeds → claimed_leaves.insert(leaf_index)  │
│     - HTLCRefund succeeds → claimed_leaves.insert(leaf_index)  │
│     - Before each operation, check leaf_index ∉ claimed_leaves │
│                                                               │
│  5. After window expires, anyone can call FinalizeSettlement:  │
│     Unclaimed amount = deposit_a + deposit_b - total_claimed   │
│     Refund ratio:                                              │
│       vault_a refund = unclaimed amount × (deposit_a / total stake) │
│       vault_b refund = unclaimed amount × (deposit_b / total stake) │
│     Channel status → Closed                                    │
└──────────────────────────────────────────────────────────────┘
```

### 5.3 Unclaimed Fund Refund Rules

`deposit_a` and `deposit_b` record the total funds injected by each party when the channel was opened. After the settlement window expires, the program returns unclaimed funds in proportion to the initial deposits:

```rust
/// Refund logic in FinalizeSettlement
fn finalize(channel: &mut ChannelAccount) {
    let total_deposit = channel.deposit_a + channel.deposit_b;
    let unclaimed = total_deposit.saturating_sub(channel.total_claimed);

    if unclaimed > 0 && total_deposit > 0 {
        // Refund in proportion to initial deposits
        let refund_a = unclaimed * channel.deposit_a / total_deposit;
        let refund_b = unclaimed.saturating_sub(refund_a);

        transfer(channel.vault_a, refund_a);
        transfer(channel.vault_b, refund_b);
    }

    channel.status = ChannelStatus::Closed;
}
```

> **Why refund proportionally**: The on-chain program cannot determine which party each unclaimed leaf originally belonged to. Refunding in proportion to initial deposits is a fair approximation — assuming each party's share in unclaimed leaves is proportional to their initial contribution. For dispute settlement, honest participants ensure they claim all leaves belonging to them, leaving no disputes.

---

## 6. Regulatory and Compliance Implementation Recommendations (Non-ZKP Path)

Since intermediate nodes serve only as routers, compliance is silently achieved through:
* **Audit Stubs**: All signed LeafUpdates are retained as local snapshots. Each LeafUpdate contains sequence, leaf_index, prev_leaf_hash, new_leaf, forming a complete audit chain of changes.
* **Quota Monitoring**: Add constraints in off-chain business logic. When cumulative payment amounts trigger thresholds, automatically insert a "compliance marker" in the next UTXO update or pause the service pending compliance review.
* **Leaf Type Extension**: A `Compliance` type can be added to `LeafType` to mark UTXOs that have completed compliance review. During settlement, the program can verify this marker.

---

## 7. Complete Transaction Sequence Diagram

```
User (Client)                           Provider                    Solana On-chain
    │                                        │                              │
    │  1. OpenChannel(Root_init, sig_a)       │                              │
    │  (User unilateral stake, initializes single-leaf Root)                │
    │ ──────────────────────────────────────────────────────────────────────>│
    │                                        │                     Create ChannelAccount
    │  <── channel_id, seq=0 ──────────────────────────────── Stake SPL Token
    │                                        │                              │
    │  2. Off-chain negotiation to build Tree │                              │
    │  (Send leaf plaintext → Provider verifies → Dual-sign Root_1)          │
    │  <─────────────────────────────────────>│                              │
    │                                        │                              │
    │  3. Batch signing:                      │                              │
    │  LeafUpdate(seq=2, UTXO_0→provider)    │                              │
    │  LeafUpdate(seq=3, UTXO_1→provider)    │                              │
    │  LeafUpdate(seq=4, UTXO_2→HTLC,H=...) │                              │
    │  LeafUpdate(seq=5, UTXO_3→provider)    │                              │
    │ ──────────────────────────────────────>│                              │
    │                                        │ Verify in order, update local Tree
    │                                        │ Root_1→Root_2→Root_3→Root_4→Root_5
    │                                        │                              │
    │                                        │ Provide AI output + R        │
    │  <──────────────────────────────────────│                              │
    │                                        │                              │
    │  LeafUpdate(seq=6, UTXO_2 HTLC→Std,   │                              │
    │             owner=provider)             │                              │
    │ ──────────────────────────────────────>│                              │
    │                                        │                              │
    │  ··· Continue using until channel balance is insufficient ···          │
    │                                        │                              │
    │  CooperativeSettle(Root_latest, seq,   │                              │
    │                    sig_a, sig_b)        │                              │
    │ ──────────────────────────────────────────────────────────────────────>│
    │                                        │                     Contract verifies dual signatures
    │                                        │                     Enter Settling
    │                                        │                              │
    │  Claim(leaf_0, proof_0) → provider     │                              │
    │ ──────────────────────────────────────────────────────────────────────>│
    │  Claim(leaf_1, proof_1) → provider     │                              │
    │ ──────────────────────────────────────────────────────────────────────>│
    │  Claim(leaf_rest, proof_rest) → user   │                              │
    │ ──────────────────────────────────────────────────────────────────────>│
    │                                        │                              │
    │  FinalizeSettlement                    │                              │
    │ ──────────────────────────────────────────────────────────────────────>│
    │                                        │                     Channel Closed
    │ <──────── Distribute funds to Token Accounts ──────────────────────────│
```

---

## 8. Development Roadmap (Milestones)

1.  **Phase 1 (Infrastructure)**: Define UTXO serialization protocol (borsh), implement Merkle Tree library (supporting leaf add/delete/modify + proof generation + proof verification), define LeafUpdate message format and signing protocol.
2.  **Phase 2 (Solana Program)**: Implement ChannelAccount structure, OpenChannel, CooperativeSettle, TriggerChallenge, SubmitCounterState, SettleAfterTimeout. Complete on-chain Merkle Proof verification. Test funding and withdrawal on devnet.
3.  **Phase 3 (Pipeline Client)**: Develop client SDK supporting batch signing, implement UTXO pre-allocation strategy and pipeline LeafUpdate generation. Integrate with provider for sequential verification. Implement UTXO split/merge operations.
4.  **Phase 4 (HTLC & Off-chain Settlement)**: Implement VerifyHTLC, HTLCRefund instructions. Integrate with AI Gateway, implement automated closed loop from token consumption to hash locking. Complete HTLC lifecycle management (creation, unlock, timeout).
5.  **Phase 5 (Hub Registration & Routing)**: Implement Hub registration program (HubLeaf + Merkle Tree), Hub metrics collection and penalty mechanism. Develop routing service (path discovery + scoring algorithm). Implement multi-hop same Hash-Lock HTLC.
6.  **Phase 6 (Optimization & Productionization)**: Implement Re-compacting (merge spent UTXOs), channel auto-renewal, liquidity rebalancing. Complete Watchtower monitoring, off-chain data backup (IPFS/Arweave).

---

## 9. Technical Risk Notes

* **Data Availability**: Both parties must each maintain a complete local copy of the Merkle Tree. If one party loses data and the other maliciously submits a stale Root, the data-losing party cannot construct a valid counterproof. It is recommended to periodically write Root change snapshots to IPFS or Arweave as backup.
* **State Ordering**: The sequence of LeafUpdate must be strictly increasing. The provider should reject out-of-order messages. If network issues cause out-of-order arrival, the provider should buffer and reorder by sequence.
* **Storage Pressure**: Persistent channels generate a large number of historical LeafUpdates. A "UTXO merge" logic is needed: merge multiple spent (owner=provider) small UTXOs into one leaf, freeing leaf slots for subsequent use. Merge operations require dual-signature confirmation.
* **Challenge Period & HTLC Timing**: The challenge period `challenge_duration` must be > the longest HTLC `timelock_slot - current_slot`, otherwise the provider will not have enough time to submit R on-chain.
* **Front-running Attack**: Solana slot time is extremely short (~400ms), a malicious party could insert a TriggerChallenge within the same slot as a previous transaction. The `ChannelAccount.min_challenge_delay` field requires at least N slots between Open → Challenge, mitigating this attack.
* **Permanent Fund Lockup**: If both parties go offline simultaneously and the channel has no auto-expiry mechanism, funds will be permanently locked. The `ChannelAccount.auto_close_slot` field provides an auto-expiry mechanism; after expiry, anyone can trigger settlement.

---

## 10. Multi-hop Routing & Hub Network

Users and merchants typically do not have a direct channel. Payments need to be relayed through multiple Hub nodes. This chapter describes the cross-channel HTLC routing mechanism and Hub registration governance system.

### 10.1 Network Topology

```
┌─────────────────────────────────────────────────────────────────────────┐
│                                                                         │
│    User (Alice)          Hub_A           Hub_B          Merchant        │
│    did:alice            did:hub_a       did:hub_b      did:merchant    │
│                                                                         │
│    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐       │
│    │ Channel  │    │ Channel  │    │ Channel  │    │ Channel  │       │
│    │ Alice-   │────│ Hub_A-   │────│ Hub_B-   │────│ Merchant │       │
│    │ Hub_A    │    │ Hub_B    │    │ Merchant │    │          │       │
│    └──────────┘    └──────────┘    └──────────┘    └──────────┘       │
│                                                                         │
│    Payment path: Alice → Hub_A → Hub_B → Merchant                      │
│    3 channel segments, requires cross-channel HTLC forwarding           │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

**Role Definitions**:

| Role | Description | Requirements |
|:-----|:------------|:-------------|
| User | Payment initiator, deposits funds into channel | DID identity verification |
| Hub | Relay routing node, provides liquidity for multiple channels | DID + platform registration + collateral + SLA |
| Merchant | Service provider, receives payments | DID + platform registration |

### 10.2 Hub Registration & Governance

Hubs serve as relay nodes; both users and merchants depend on their uptime and fund adequacy. The platform must conduct admission review and ongoing supervision of Hubs.

#### 10.2.1 Hub Registration Flow

```
Hub Operator                          Platform                       Solana On-chain
    │                                       │                              │
    │  1. Submit registration application    │                              │
    │  { did: "did:ignite:hub_xxx",          │                              │
    │    endpoint: "wss://hub.example.com",  │                              │
    │    supported_tokens: [USDC, USDT],     │                              │
    │    max_channel_capacity: $100000 }      │                              │
    │ ──────────────────────────────────────>│                              │
    │                                       │                              │
    │                                       │  2. Platform review:          │
    │                                       │     - DID document completeness ✓│
    │                                       │     - Endpoint availability test ✓│
    │                                       │     - KYB (business verification) ✓│
    │                                       │                              │
    │  3. Hub deposits collateral            │                              │
    │ ──────────────────────────────────────────────────────────────────────>│
    │                                       │              Lock collateral to PDA
    │                                       │              Record HubLeaf into
    │                                       │              Merkle Tree
    │                                       │                              │
    │  <── Registration successful, hub_id ─│                              │
```

#### 10.2.2 Hub On-chain Registration Data

Hub registration information is stored in a platform-managed Merkle Tree (reusing SPL Account Compression infrastructure):

```rust
/// Hub registration leaf — stored in platform Merkle Tree
struct HubLeaf {
    /// Hash of the Hub's DID
    hub_did_hash: [u8; 32],

    /// Hub's current active public key (for communication and signature verification)
    active_pubkey: Pubkey,

    /// Communication endpoint hash (SHA-256 of "wss://hub.example.com")
    endpoint_hash: [u8; 32],

    /// Collateral amount (smallest token unit)
    collateral: u64,

    /// Hash of platform-issued VC (proves review passed)
    platform_vc_hash: [u8; 32],

    /// Service metrics (updated off-chain, hash anchored on-chain)
    /// SHA-256 of { online_rate, success_rate, avg_latency, total_routed }
    metrics_hash: [u8; 32],

    /// Last updated slot
    slot_updated: u64,
}
```

#### 10.2.3 Hub Metrics & Rating System

Users reference the following metrics when selecting a Hub:

```rust
/// Hub service metrics — maintained off-chain, periodically hashed on-chain
struct HubMetrics {
    /// Online rate (0~10000, representing 0.00%~100.00%)
    online_rate: u16,

    /// Payment success rate (0~10000)
    success_rate: u16,

    /// Average routing latency (milliseconds)
    avg_latency_ms: u32,

    /// Cumulative routed amount (smallest token unit)
    total_routed: u64,

    /// Cumulative routed transaction count
    total_transactions: u64,

    /// Current active channel count
    active_channels: u32,

    /// Available liquidity (maximum amount currently routable)
    available_liquidity: u64,

    /// Fee rate (basis points, e.g. 10 = 0.1%)
    fee_rate_bps: u16,
}
```

**Metrics Update Flow**:

```
1. Platform Watchtower probes Hub endpoint availability every T seconds
2. Hub reports routing result after each route completion (success/failure/latency)
3. Platform aggregates metrics, computes metrics_hash
4. Platform calls update_hub_leaf to update Merkle Tree
5. When users query a Hub, verify the on-chain proof corresponding to metrics_hash
```

**Penalty Mechanism**:

| Violation | Penalty |
|:----------|:--------|
| Online rate < 99% for 7 consecutive days | Deduct 10% collateral |
| Routing success rate < 95% for 3 consecutive days | Deduct 5% collateral |
| Malicious withholding of HTLC preimage | Deduct all collateral + permanent ban |
| Insufficient funds causing routing failure | Warning + deduct 5% collateral after 3 occurrences |

### 10.3 Route Discovery

Before initiating a payment, the user needs to find an available route from themselves to the merchant.

#### 10.3.1 Route Query Flow

```
User (Alice)                  Route Server                    On-chain/Index
    │                               │                              │
    │  1. Route request              │                              │
    │  { from: "did:alice",          │                              │
    │    to: "did:merchant",         │                              │
    │    amount: $0.05,              │                              │
    │    token: USDC }               │                              │
    │ ─────────────────────────────>│                              │
    │                               │                              │
    │                               │  2. Query channel graph:      │
    │                               │  - Alice's active channels     │
    │                               │  - Channels reachable to Hubs  │
    │                               │  - Merchant's active channels  │
    │                               │                              │
    │                               │  3. Query Hub metrics          │
    │                               │  (verified from on-chain Merkle Tree)│
    │                               │ ────────────────────────────>│
    │                               │                              │
    │                               │  4. Compute candidate routes:  │
    │                               │  Path_1: Alice→HubA→Merchant   │
    │                               │    fee: 0.05%, latency: 50ms  │
    │                               │    liquidity: ✓                │
    │                               │  Path_2: Alice→HubB→HubC→M    │
    │                               │    fee: 0.08%, latency: 80ms  │
    │                               │    liquidity: ✓                │
    │                               │                              │
    │  5. Return candidate routes    │                              │
    │  <─────────────────────────────│                              │
    │  [Path_1 (recommended), Path_2]│                              │
    │                               │                              │
    │  6. User selects Path_1        │                              │
```

#### 10.3.2 Route Selection Algorithm

```rust
/// Route scoring — comprehensively considering fees, latency, reliability, liquidity
fn score_route(
    path: &[RouteHop],
    amount: u64,
    hub_metrics: &HashMap<Pubkey, HubMetrics>,
) -> f64 {
    let mut total_fee: u64 = 0;
    let mut max_latency: u32 = 0;
    let mut min_success_rate: u16 = 10000;
    let mut min_liquidity: u64 = u64::MAX;

    for hop in path {
        let metrics = hub_metrics.get(&hop.hub_pubkey);
        if let Some(m) = metrics {
            total_fee += amount * m.fee_rate_bps as u64 / 10000;
            max_latency = max_latency.max(m.avg_latency_ms);
            min_success_rate = min_success_rate.min(m.success_rate);
            min_liquidity = min_liquidity.min(m.available_liquidity);
        }
    }

    // Liquidity must be sufficient
    if min_liquidity < amount + total_fee {
        return f64::NEG_INFINITY;
    }

    // Weighted score (higher is better)
    let fee_score = 1.0 / (1.0 + total_fee as f64 / amount as f64);
    let latency_score = 1.0 / (1.0 + max_latency as f64 / 1000.0);
    let reliability_score = min_success_rate as f64 / 10000.0;

    0.3 * fee_score + 0.3 * latency_score + 0.4 * reliability_score
}
```

### 10.4 Cross-channel HTLC Routing (Multi-hop HTLC)

Cross-channel payments use nested HTLCs to achieve atomicity: all channels use the same preimage R, or use the same hash_lock chain.

#### 10.4.1 Same Hash-Lock Multi-hop (Recommended)

All channels use the same `hash_lock = SHA-256(R)`, where R is generated by the final merchant. Once any party obtains R, they can unlock all upstream channels.

```
Route: Alice → Hub_A → Hub_B → Merchant

Step 1: Merchant generates R, computes H = SHA-256(R)

Step 2: Lock HTLC from merchant side backwards (reverse construction)

  Merchant → Hub_B:
    In Hub_B-Merchant channel:
    UTXO_i: HTLC, $0.05, hash_lock=H, owner=Hub_B, beneficiary=Merchant
    (Hub_B locks funds, Merchant is beneficiary. Timeout returns to Hub_B)

  Hub_B → Hub_A:
    In Hub_A-Hub_B channel:
    UTXO_j: HTLC, $0.05+fee_B, hash_lock=H, owner=Hub_A, beneficiary=Hub_B
    (Hub_A locks funds, Hub_B is beneficiary. Timeout returns to Hub_A)

  Hub_A → Alice:
    In Alice-Hub_A channel:
    UTXO_k: HTLC, $0.05+fee_A+fee_B, hash_lock=H, owner=Alice, beneficiary=Hub_A
    (Alice locks funds, Hub_A is beneficiary. Timeout returns to Alice)

Step 3: Alice confirms route and HTLC conditions, signs LeafUpdate to lock UTXO_k as HTLC
  (At this point Alice does not know R, but she trusts the HTLC mechanism: only Hub_A can obtain funds by providing R)

Step 4: Hub_A receives Alice's HTLC confirmation, signs LeafUpdate to lock UTXO_j as HTLC

Step 5: Hub_B receives Hub_A's HTLC confirmation, signs LeafUpdate to lock UTXO_i as HTLC

Step 6: Merchant receives payment, reveals R to all upstream nodes
  Merchant → Hub_B → Hub_A → Alice: broadcast R

Step 7: All nodes use R to complete their respective HTLC unlocks
```

**Sequence Diagram**:

```
Alice          Hub_A          Hub_B          Merchant
  │              │              │              │
  │  1. Route request + H       │              │
  │─────────────>│              │              │
  │              │  2. Forward + H              │
  │              │─────────────>│              │
  │              │              │  3. Forward + H│
  │              │              │─────────────>│
  │              │              │              │
  │              │              │  4. Hub_B channel: HTLC owner=Hub_B, beneficiary=Merchant
  │              │              │<─────────────│
  │              │              │              │  UTXO: HTLC, $0.05, H
  │              │  5. Hub_A channel: HTLC owner=Hub_A, beneficiary=Hub_B
  │              │<─────────────│              │
  │              │              │              │  UTXO: HTLC, $0.05+fee_B, H
  │  6. Alice channel: HTLC owner=Alice, beneficiary=Hub_A
  │<─────────────│              │              │
  │              │              │              │  UTXO: HTLC, $0.05+fee_A+B, H
  │              │              │              │
  │  7. Alice confirms payment  │              │
  │─────────────>│              │              │
  │              │  8. Hub_A forwards           │
  │              │─────────────>│              │
  │              │              │  9. Hub_B forwards│
  │              │              │─────────────>│
  │              │              │              │
  │              │              │  10. Merchant reveals R
  │              │              │<─────────────│
  │              │  11. R propagates            │
  │              │<─────────────│              │
  │  12. R propagates           │              │
  │<─────────────│              │              │
  │              │              │              │
  │  13. All parties use R to unlock HTLC      │
  │  ✓           │  ✓           │  ✓           │
```

#### 10.4.2 Cross-channel HTLC Timing Constraints

Multi-hop HTLC involves multiple channels; each channel's timelock_slot must satisfy a decreasing constraint:

```
Constraint: From payer to payee, timelock_slot strictly decreases

  Alice-Hub_A channel:     timelock_slot = T
  Hub_A-Hub_B channel:     timelock_slot = T - Δ
  Hub_B-Merchant channel:  timelock_slot = T - 2Δ

Where Δ = SAFETY_MARGIN (e.g. 1000 slots ≈ 6.7 minutes)

Reason:
  If Merchant does not reveal R, Alice's HTLC times out first, Alice can refund
  Then upstream channels time out and refund in order
  Intermediate Hubs have enough time to obtain R from their downstream channel before submitting to upstream

  If timing is reversed (Merchant's timelock expires first):
  Merchant times out and refunds (owner=Hub_B returns), but Hub_B still holds R
  Hub_B as beneficiary claims funds in Hub_A-Hub_B channel using R
  → Hub_B simultaneously recovers Hub_B-Merchant channel refund (as owner) + Hub_A-Hub_B channel funds (as beneficiary)
  → Hub_B gains double, fund security is broken
```

**Recommended Parameters**:

```
HOP_MARGIN = 1000 slots  (approximately 6.7 minutes, safety margin per hop)
MIN_TIMELOCK = CHALLENGE_DURATION + 3 * HOP_MARGIN  (minimum HTLC duration)

Timelock settings for 3-hop routing:
  Alice-Hub_A:     current_slot + MIN_TIMELOCK + 2 * HOP_MARGIN
  Hub_A-Hub_B:     current_slot + MIN_TIMELOCK + HOP_MARGIN
  Hub_B-Merchant:  current_slot + MIN_TIMELOCK
```

#### 10.4.3 Route Failure Handling

```
Scenario 1: Hub has insufficient liquidity
  Hub_B cannot lock enough funds in Hub_B-Merchant channel
  → Hub_B returns RouteError("InsufficientLiquidity") to Hub_A
  → Hub_A tries alternative routes or returns to Alice
  → Alice selects backup route

Scenario 2: HTLC timeout (a Hub goes offline)
  Hub_A goes offline after receiving Alice's payment
  → Alice's HTLC automatically refunds after timelock_slot expires
  → Hub_A-Hub_B channel HTLC also times out and refunds
  → Funds are safe, no one loses (except time)

Scenario 3: Intermediate node maliciously withholds R
  Hub_B receives R from Merchant but does not forward to Hub_A

  Analysis:
  Hub_B is the beneficiary in Hub_B-Merchant channel; after obtaining R, can claim Merchant's funds.
  But in Hub_A-Hub_B channel HTLC, Hub_B is the beneficiary, and Hub_A cannot obtain R.
  After Hub_A's HTLC times out, Hub_A refunds as owner.

  Hub_B's result:
  - Hub_B-Merchant channel: claimed Merchant's $0.05 using R ✓ (legitimate benefit)
  - Hub_A-Hub_B channel: claimed Hub_A's locked $0.05+fee_B using R as beneficiary ✓

  Hub_B's total gain = $0.05 (Merchant channel) + $0.05+fee_B (Hub_A channel)
  But Hub_B in the normal flow would have:
  - Hub_B-Merchant channel: $0.05 (routing fee already included)
  - Hub_A-Hub_B channel: $0.05+fee_B (routing fee revenue)

  Conclusion: Hub_B's financial balance is identical to the normal flow. Hub_B withholding R does not affect fund security,
  it only causes Hub_A's refund to go through the on-chain timeout path (rather than immediate off-chain unlock), increasing latency.

  Defense: No defense needed. Fund security is not affected.
  The only consequence of Hub_B withholding R is increased on-chain transactions (Hub_A goes through timeout refund), increasing cost but no fund loss.
```

### 10.5 Routing Fee Settlement

Hubs profit from routing fees. Routing fees are implicitly settled through HTLC amount differences in each channel:

```
User pays: $0.05
Routing fees: Hub_A 0.01%, Hub_B 0.02%

Locked amounts per channel:
  Alice-Hub_A:     $0.05 × (1 + 0.01% + 0.02%) = $0.050015
  Hub_A-Hub_B:     $0.05 × (1 + 0.02%)          = $0.050010
  Hub_B-Merchant:  $0.05                          = $0.050000

After HTLC unlock:
  Alice → Hub_A:     $0.050015 (Hub_A earns $0.000005 routing fee)
  Hub_A → Hub_B:     $0.050010 (Hub_B earns $0.000010 routing fee)
  Hub_B → Merchant:  $0.050000 (Merchant receives $0.05)
```

Routing fees are implemented within the channel through the normal HTLC → Standard LeafUpdate flow, requiring no additional on-chain operations.

### 10.6 Hub & User Channel Management

#### 10.6.1 User-Hub Channel Opening Process

```
1. User queries candidate Hub list (obtained from routing service)
2. User selects a Hub (based on online rate, fee rate, liquidity)
3. User verifies Hub's on-chain registration info:
   a. HubLeaf Merkle Proof verification passes
   b. platform_vc_hash is valid
   c. collateral >= minimum requirement
   d. online rate in metrics_hash >= 99%
4. User first submits OpenChannel on-chain transaction (single-party deposit)
5. User initiates off-chain negotiation with Hub (sends channel_id + denomination plan)
6. Both parties negotiate UTXO denominations off-chain → construct Tree → dual-sign Root_1
7. Channel is ready, payments can begin
```

#### 10.6.2 Inter-Hub Channel Management

Hubs need to pre-establish channels to provide liquidity. Dual-funded channels also follow the "on-chain first, then off-chain" principle:

```
Hub_A                                Hub_B
  │                                     │
  │  1. Negotiate channel parameters     │
  │  (Dual funding, each contributes $5000,
  │   denominations: 25×$50(A) + 25×$50(B) + 25×$100(A) + 25×$100(B) + Rest_A + Rest_B) │
  │                                     │
  │  2. Hub_A submits OpenChannel on-chain transaction first
  │  (Single-party deposit $5000, Root_init = all belongs to Hub_A)
  │                                     │
  │  3. Hub_B submits FundChannel on-chain transaction
  │  (Deposit $5000, deposit_b = $5000) │
  │                                     │
  │  4. Off-chain negotiation to construct Tree:
  │  Hub_A locally constructs split Tree:
  │     UTXO_0~24:  $50, owner=Hub_A    │
  │     UTXO_25~49: $50, owner=Hub_B    │
  │     UTXO_50~74: $100, owner=Hub_A   │
  │     UTXO_75~99: $100, owner=Hub_B   │
  │     UTXO_100 (Rest_A): $1250, owner=Hub_A
  │     UTXO_101 (Rest_B): $1250, owner=Hub_B
  │     Compute Root_1                   │
  │                                     │
  │  Hub_A total: 25×$50 + 25×$100 + $1250 = $5000 ✓
  │  Hub_B total: 25×$50 + 25×$100 + $1250 = $5000 ✓
  │  Grand total: $10000 ✓               │
  │                                     │
  │  5. Hub_A sends leaf plaintext + Root_1
  │ ──────────────────────────────────>│
  │                                     │
  │  6. Hub_B verifies:                  │
  │     a. Locally build Tree, compare Root_1
  │     b. Verify Hub_A leaf total (including Rest_A) == $5000
  │     c. Verify Hub_B leaf total (including Rest_B) == $5000
  │     d. Verify total amount == $10000 │
  │                                     │
  │  7. Both parties sign Root_1 (seq=1) │
  │                                     │
  │  ========== Dual channel ready ===========
```

> **Note**: In dual-funded channels, both parties make on-chain deposits first, then negotiate allocation off-chain. Each Hub holds an independent Rest leaf (Rest_A / Rest_B), ensuring each party's leaf amount sum exactly equals their respective deposit amount. If off-chain negotiation fails, both parties can fully refund based on on-chain Root_init.

#### 10.6.3 Liquidity Rebalancing

During channel usage, funds flow in one direction (e.g. user continuously pays Hub), causing one party's UTXOs to be depleted:

```
Scenario: In Alice-Hub_A channel, Alice's spendable UTXOs are depleted

Detection: Alice locally finds all leaves with owner=Alice and type=Standard have amount=0

Option 1: Off-chain rebalancing (via another channel)
  Alice receives a refund through Hub_B's channel, then routes via Hub_B→Hub_A
  to transfer funds back to Hub_A channel (requires Hub_A to have liquidity in Hub_B channel)

Option 2: On-chain top-up (simple but requires on-chain transaction)
  1. Alice adds deposit to existing channel
  2. Both parties negotiate new UTXO allocation
  3. Sign a batch of LeafUpdates to reallocate leaves
  4. Submit UpdateChannel on-chain transaction to update current_root and deposit_a

Option 3: Close old channel, open new channel
  (Simplest but highest cost)
```

---

## 11. FLOW-1/2/3/7 Implementation Specification

### 11.1 FLOW-3: Dual-funded Channels

#### 11.1.1 fund_channel Method

```rust
pub fn fund_channel(
    &self,
    state: &mut ChannelState,
    provider_keypair: &Keypair,
    deposit_b: u64,
    provider_leaf_index: Option<usize>,  // None = auto-select first empty slot
) -> Result<()>
```

**Validation Rules**:
- Channel status must be `Open`
- `deposit_b` must currently be 0 (not yet funded)
- Signer must be the channel's provider
- `deposit_b` must be greater than 0
- An available empty leaf slot must exist

**Execution Flow**:
1. Select an empty slot in the Merkle tree (auto or specified)
2. Create a Standard leaf owned by provider
3. Sign the update via `sign_leaf_update`
4. Apply leaf update to the tree
5. Update `deposit_b`, `total_deposited` (saturating_add)
6. Persist state to sled

#### 11.1.2 construct_split_tree Extension

The original implementation required all non-empty leaves to be owned by user. FLOW-3 extends to:
- Allow leaf owner to be user or provider
- Add per-party amount validation: `user_total == deposit_a`, `provider_total == deposit_b`
- Total amount conservation still holds: `user_total + provider_total == total_deposited`

#### 11.1.3 On-chain FundChannel Instruction

SPL Token flow:
1. Provider calls `fund_channel` instruction
2. Verify signer == channel.provider_pubkey
3. Transfer deposit_b from source_vault to vault_b via SPL Token CPI
4. Update ChannelAccount's deposit_b and total_deposited

### 11.2 FLOW-7: Compliance Module

#### 11.2.1 Data Structures

```rust
SpendingLimit {
    threshold: u64,      // Cumulative spending threshold
    per_channel: u64,    // Per-channel limit
    window_slots: u64,   // Rolling window
}

TravelRuleData {
    originator_id: Vec<u8>,
    beneficiary_id: Vec<u8>,
    amount: u64,
    created_slot: u64,
    channel_id: [u8; 32],
    originator_jurisdiction: Vec<u8>,
    beneficiary_jurisdiction: Vec<u8>,
}

ComplianceAction {
    None | InsertMarker { compliance_hash, threshold }
}
```

#### 11.2.2 ComplianceManager API

| Method | Description |
|--------|-------------|
| `new(db)` | Create compliance manager |
| `init_channel_compliance(channel_id, limits)` | Initialize channel compliance state |
| `load_state(channel_id)` | Load channel compliance state |
| `record_payment(channel_id, amount, slot, user, provider)` | Record payment and check threshold |
| `clear_hold(channel_id)` | Clear compliance hold |
| `record_audit(LeafUpdate)` | Record audit log |
| `get_audit_trail(channel_id)` | Get audit trail |
| `create_compliance_leaf(hash)` | Create compliance marker leaf |

**Sled Key Format**:
- `compliance:{hex(channel_id)}` → ChannelComplianceState
- `audit:{hex(channel_id)}:{sequence}` → LeafUpdate

#### 11.2.3 Spending Limit Logic

When `cumulative_spent >= threshold`:
1. Set `compliance_hold = true`
2. Return `ComplianceAction::InsertMarker`
3. Channel pauses until `clear_hold` is called

### 11.3 FLOW-2: Multi-hop Routing

#### 11.3.1 Constants

```rust
pub const HOP_MARGIN: u64 = 1000;     // ~6.7 minutes per hop
pub const MIN_TIMELOCK_BASE: u64 = 500 + 3 * HOP_MARGIN;  // Base timelock
```

#### 11.3.2 HubManager API

| Method | Description |
|--------|-------------|
| `new(db)` | Create Hub manager |
| `register_hub(HubLeaf)` | Register Hub |
| `get_hub(did_hash)` | Query Hub |
| `get_metrics(did_hash)` | Get metrics |
| `update_metrics(did_hash, HubMetrics)` | Update metrics |
| `list_hubs()` | List all Hubs |
| `compute_metrics_hash(metrics)` | Compute metrics hash |

**Sled Key Format**:
- `hub:{hex(did_hash)}` → HubLeaf
- `hub_metrics:{hex(did_hash)}` → HubMetrics

#### 11.3.3 RouteService API

| Method | Description |
|--------|-------------|
| `new(hub_manager)` | Create routing service |
| `refresh_graph()` | Refresh channel graph from Hub registry |
| `discover_routes(req)` | Discover all routes |
| `score_route(metrics, amount)` | Score route |
| `select_best_route(routes)` | Select best route |

**Scoring Formula**: `0.3 * fee_score + 0.3 * latency_score + 0.4 * reliability_score`

#### 11.3.4 MultiHopManager API

| Method | Description |
|--------|-------------|
| `new(db)` | Create multi-hop manager |
| `create_payment(hash_lock, preimage, hops_metadata, current_slot)` | Create payment |
| `create_htlc_leaf_update(hop, sequence, prev_leaf, signer)` | Create HTLC leaf update |
| `reveal_preimage(payment_id, preimage)` | Reveal preimage |
| `resolve_hop(payment_id, hop_index)` | Resolve single hop |
| `check_expiry(payment_id, current_slot)` | Check expiry |
| `load_payment / persist_payment` | Persist |

**Timelock Formula**:
```
hop[i].timelock = base_timelock - i * HOP_MARGIN
base_timelock = current_slot + MIN_TIMELOCK_BASE + (num_hops - 1) * HOP_MARGIN
```

**Sled Key Format**:
- `multihop:{hex(payment_id)}` → MultiHopPayment

### 11.4 FLOW-1: On-chain Solana Program

#### 11.4.1 Program Structure

```
ignite-pay-program/
├── Cargo.toml          (anchor-lang 0.30, anchor-spl 0.30)
├── Anchor.toml
└── src/
    ├── lib.rs           (10 instruction entry points)
    ├── state.rs         (ChannelAccount, ChannelStatus)
    ├── error.rs         (ChannelError enum)
    ├── instructions/
    │   ├── open_channel.rs
    │   ├── fund_channel.rs
    │   ├── cooperative_settle.rs
    │   ├── trigger_challenge.rs
    │   ├── submit_counter_state.rs
    │   ├── settle_after_timeout.rs
    │   ├── claim.rs
    │   ├── verify_htlc.rs
    │   ├── htlc_refund.rs
    │   └── finalize_settlement.rs
    └── utils/
        └── merkle.rs    (verify_merkle_proof sorted-pair)
```

#### 11.4.2 ChannelAccount Layout

| Field | Type | Size |
|-------|------|------|
| discriminator | [u8; 8] | 8 |
| channel_id | [u8; 32] | 32 |
| user_pubkey | Pubkey | 32 |
| provider_pubkey | Pubkey | 32 |
| token_mint | Pubkey | 32 |
| status | ChannelStatus (enum) | 1 + padding |
| sequence | u64 | 8 |
| current_root | [u8; 32] | 32 |
| total_deposited | u64 | 8 |
| open_slot | u64 | 8 |
| challenge_slot | Option\<u64\> | 1 + 8 |
| vault_a | Pubkey | 32 |
| vault_b | Pubkey | 32 |
| deposit_a | u64 | 8 |
| deposit_b | u64 | 8 |
| challenge_duration | u64 | 8 |
| min_challenge_delay | u64 | 8 |
| total_claimed | u64 | 8 |
| settle_deadline | Option\<u64\> | 1 + 8 |
| tree_depth | u32 | 4 |

#### 11.4.3 Instruction Signatures

| # | Instruction | Signature Message Family | Signer | Parameters | Accounts |
|---|-------------|-------------------------|--------|------------|----------|
| 1 | open_channel | **Family A** | User single-sign | channel_id, deposit_a, tree_depth, open_slot, challenge_duration, min_challenge_delay, initial_root | channel (init), user_pubkey, provider_pubkey, token_mint, vault_a, vault_b, payer |
| 2 | fund_channel | — | No signature | deposit_b | channel (mut), signer (provider), source_vault, vault_b |
| 3 | cooperative_settle | **Family B** | Dual-sign sig_a+sig_b | sequence, root, settle_window, sig_a, sig_b | channel (mut), clock |
| 4 | trigger_challenge | **Family C** | Submitter single-sign | submitted_sequence, submitted_root, challenger_signature | channel (mut), challenger, clock |
| 5 | submit_counter_state | **Family B** | Dual-sign sig_a+sig_b | sequence, root, sig_a, sig_b | channel (mut) |
| 6 | settle_after_timeout | — | No signature | settle_window | channel (mut), clock |
| 7 | claim | **Family C** | Claimant single-sign | leaf_index, claim_amount, leaf_owner, leaf_hash, proof, claimer_signature | channel (mut), claimer, clock |
| 8 | verify_htlc | **Family C** | Claimant single-sign | leaf_index, preimage, hash_lock, leaf_amount, beneficiary, leaf_hash, proof, claimer_signature | channel (mut), claimer, clock |
| 9 | htlc_refund | **Family C** | Claimant single-sign | leaf_index, timelock_slot, leaf_amount, leaf_owner, leaf_hash, proof, claimer_signature | channel (mut), claimer, clock |
| 10 | finalize_settlement | **Family C** | Caller single-sign | caller_signature | channel (mut), caller, vault_a, vault_b, escrow_vault, clock |

Signature message family definitions are in section 4.3.

#### 11.4.4 CPI Details

- **FundChannel**: SPL Token `transfer` CPI from provider source → vault_b
- **FinalizeSettlement**: SPL Token `transfer` CPI from escrow → vault_a / vault_b (proportional)
- **Ed25519 Signature Verification**: Via Solana ed25519_program instruction introspection, executed before the main instruction
- **Merkle Proof**: Uses `hashv(&[min, max])` sorted-pair pattern for verification
