# Scenario 9: Multi-hop Payment

## 1. Scenario Description

A user initiates a payment to a remote merchant through multiple Hub relays. Atomicity is achieved using an HTLC chain with decreasing timelocks: either all hops complete, or all roll back. Each Hub relay charges a fee rate.

## 2. Participants

| Role | Responsibility |
|:-----|:-----|
| User | Initiate multi-hop payment, lock first-hop HTLC |
| Hub 1..N | Relay payment, lock/unlock HTLC |
| Provider (Merchant) | Final payee, reveal preimage to trigger backward resolution |

## 3. Prerequisites

- Route has been discovered (see Scenario 8)
- Each hop channel is open with sufficient liquidity
- Shared hash_lock + preimage has been generated

## 4. Operation Flow

```
User           Hub1            Hub2           Provider
 │               │               │               │
 │  1. compute_hop_amounts       │               │
 │  Calculate: User→Hub1: 1001000                │
 │             Hub1→Hub2: 1000500                │
 │             Hub2→Prov: 1000000                │
 │               │               │               │
 │  2. create_payment            │               │
 │  Shared hash_lock + preimage  │               │
 │  Decreasing timelock:         │               │
 │  hop0: base_timelock          │               │
 │  hop1: base - HOP_MARGIN     │               │
 │  hop2: base - 2×HOP_MARGIN   │               │
 │               │               │               │
 │  3. Create HTLC leaf for each hop             │
 │  create_htlc_leaf_update      │               │
 │──────────────→│──────────────→│──────────────→│
 │               │               │    4. Provider
 │               │               │    reveals preimage
 │               │               │←──────────────│
 │               │               │               │
 │               │  5. Backward hop-by-hop resolution
 │←──────────────│←──────────────│               │
 │  resolve_hop  │  resolve_hop  │  resolve_hop  │
 │               │               │               │
 │       All resolved → Completed                │
```

### Timelock Calculation

```
HOP_MARGIN = 1000 slots (~6.7 minutes)
HTLC_SAFETY_MARGIN = 1000 slots

min_timelock = challenge_duration + 3 × HOP_MARGIN
base_timelock = current_slot + min_timelock + (num_hops - 1) × HOP_MARGIN

hop[i].timelock = base_timelock - i × HOP_MARGIN
```

### Fee Calculation

```rust
// compute_hop_amounts(target_amount, &[fee_rate_bps...])
// Last hop = target amount
// Each hop adds fee upward: amount[i] = amount[i+1] * (1 + fee_rate[i]/10000)
```

## 5. HTTP API Calls

### Create Multi-hop Payment

```bash
curl -X POST http://localhost:3001/v1/multihop/create \
  -H "Content-Type: application/json" \
  -d '{
    "hops": [
      {"owner": "User public key", "beneficiary": "Hub1 public key", "amount": 1001000, "leaf_index": 0, "channel_id": "hex..."},
      {"owner": "Hub1 public key", "beneficiary": "Hub2 public key", "amount": 1000500, "leaf_index": 1, "channel_id": "hex..."},
      {"owner": "Hub2 public key", "beneficiary": "Provider public key", "amount": 1000000, "leaf_index": 2, "channel_id": "hex..."}
    ],
    "current_slot": 123456789,
    "challenge_duration": 5000
  }'
```

### Resolve Single Hop

```bash
curl -X POST http://localhost:3001/v1/multihop/{payment_id}/resolve \
  -H "Content-Type: application/json" \
  -d '{"hop_index": 2}'
```

### Hub Relay

```bash
curl -X POST http://localhost:3003/v1/multihop/relay \
  -H "Content-Type: application/json" \
  -d '{"payment_id": "hex...", "hop_index": 1}'
```

### Query Payment Status

```bash
curl http://localhost:3003/v1/multihop/{payment_id}
```

## 6. Rust Library Calls

```rust
use ignite_pay_state_channel::multihop::MultiHopManager;

let multihop = MultiHopManager::new(db.clone())?;

// Create multi-hop payment
let hops_metadata = vec![
    (user_pk, hub1_pk, 1_001_000, 0, channel_id_1),
    (hub1_pk, hub2_pk, 1_000_500, 1, channel_id_2),
    (hub2_pk, prov_pk, 1_000_000, 2, channel_id_3),
];

let payment = multihop.create_payment(
    hash_lock, preimage, hops_metadata, current_slot, challenge_duration,
)?;

// Create HTLC LeafUpdate for each hop
for hop in &payment.hops {
    let update = MultiHopManager::create_htlc_leaf_update(hop, sequence, &prev_leaf, &signer_kp);
}

// Reveal preimage (final payee)
let payment = multihop.reveal_preimage(&payment_id, &preimage)?;

// Backward hop-by-hop resolution
for i in (0..payment.hops.len()).rev() {
    let payment = multihop.resolve_hop(&payment_id, i)?;
}
```

## 7. On-Chain Operations

Off-chain operations for multi-hop payments require no on-chain transactions. On-chain HTLC verification is only triggered during the settlement phase (see Scenario 7).

## 8. Error Handling

| Error | Cause | Handling |
|:-----|:-----|:-----|
| `InsufficientLiquidity` | Insufficient balance in a hop channel | Re-route or add liquidity |
| `HopExpired` | A hop timelock has expired | Payment automatically fails, process refund |
| `InvalidHopOrder` | Incorrect resolution order | Must resolve sequentially from last hop backward |
| `PreimageMismatch` | Preimage does not match hash_lock | Confirm preimage is correct |

## 9. Notes

- Multi-hop payment states: `Pending → Locked → Resolving → Completed` or `Pending → Failed`
- Decreasing timelocks ensure upstream always has enough time to refund after downstream timeout
- `HOP_MARGIN = 1000 slots` is a safety margin and must not be reduced
- On payment failure, each hop must independently process HTLC refunds
- Each hop has a different `channel_id` (cross-channel routing)

---

## Related Scenarios

| Scenario | Relationship |
|:-----|:-----|
| [01 Channel Open](01-channel-open.md) | Prerequisite: each hop requires an open channel |
| [04 HTLC Payment](04-htlc-payment.md) | Prerequisite: each hop uses HTLC mechanism |
| [07 HTLC Settlement](07-htlc-settlement.md) | On-chain settlement for each hop HTLC |
| [08 Hub Routing](08-hub-routing.md) | Prerequisite: route discovery (Scenario 8) |
