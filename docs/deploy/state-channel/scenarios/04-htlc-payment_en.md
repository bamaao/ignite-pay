# Scenario 4: HTLC Conditional Payment

## 1. Scenario Description

Using Hash Time-Locked Contract (HTLC) to implement conditional payment. Funds are locked in a UTXO leaf by hash_lock, and only the beneficiary who provides the correct preimage can claim them. If the preimage is not revealed before the timelock expires, the funds are returned to the original owner.

Applicable to: atomic swaps, conditional delivery, and as a building block for cross-channel multi-hop payments.

## 2. Participating Roles

| Role | Responsibility |
|:-----|:-----|
| User | Creates the HTLC, holds the preimage, reveals it when the condition is met |
| Provider | Verifies the hash_lock, receives the preimage after service delivery to claim funds |

## 3. Prerequisites

- Channel is open and has completed the split tree
- A standard type leaf is available for conversion to an HTLC leaf
- Both parties have agreed on the HTLC amount and timelock duration

## 4. Operation Flow

### Flow A — Normal Completion

```
User                                   Provider
 │  1. Create HTLC                       │
 │  HtlcManager::create_htlc(...)        │
 │  → (hash_lock, preimage)              │
 │                                        │
 │  2. Send hash_lock to Provider        │
 │  (preimage kept secret)                │
 │───────────────────────────────────────→│
 │                                        │
 │  3. Pipeline::create_htlc(leaf_idx,   │
 │     hash_lock, timelock, beneficiary)  │
 │  → generate signed LeafUpdate          │
 │───────────────────────────────────────→│
 │                                        │  4. Provider co-signs
 │←───────────────────────────────────────│
 │                                        │
 │  ===== Service Delivery =====          │
 │                                        │
 │  5. Reveal preimage                    │
 │  HtlcManager::reveal_preimage(...)     │
 │  Pipeline::resolve_htlc(leaf_idx,      │
 │    &preimage)                          │
 │───────────────────────────────────────→│
 │                                        │  6. Funds transferred to Provider
 │  HtlcManager::mark_fulfilled(...)      │
```

### Flow B — Timeout Refund

```
User                                   Provider
 │  ...HTLC created, timelock expired...  │
 │                                        │
 │  HtlcManager::check_expiry(slot)       │
 │  → marked as Expired                   │
 │                                        │
 │  Pipeline::refund_htlc(leaf_idx)       │
 │  → funds returned to User              │
 │                                        │
 │  HtlcManager::mark_refunded(...)       │
```

## 5. HTTP API Calls

### Create HTLC

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/htlc/create \
  -H "Content-Type: application/json" \
  -d '{
    "amount": 100000,
    "leaf_index": 2,
    "beneficiary": "Provider public key (Base58)",
    "duration": 500
  }'
```

### Resolve HTLC (Reveal Preimage)

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/htlc/resolve \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 2,
    "preimage": "hex-encoded 32-byte preimage"
  }'
```

### HTLC Refund (After Timeout)

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/htlc/refund \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 2
  }'
```

## 6. Rust Library Calls

```rust
use ignite_pay_state_channel::htlc::HtlcManager;

let htlc_mgr = HtlcManager::with_db(db.clone(), channel_id);

// Create HTLC
let (hash_lock, preimage) = htlc_mgr.create_htlc(
    100_000,           // amount
    2,                  // leaf index
    user_pubkey,        // owner
    provider_pubkey,    // beneficiary
    current_slot,       // current slot
    500,                // duration (slots)
);

// Create HTLC leaf in Pipeline
{
    let mut pipeline = Pipeline::new(&mut tree, channel_id, seq, &user_keypair);
    pipeline.create_htlc(2, hash_lock, timelock_slot, provider_pubkey, current_slot, challenge_duration)?;
    let (updates, _) = pipeline.build();
}

// Reveal preimage
htlc_mgr.reveal_preimage(&hash_lock, &preimage)?;

// Resolve HTLC in Pipeline
{
    let mut pipeline = Pipeline::new(&mut tree, channel_id, seq, &user_keypair);
    pipeline.resolve_htlc(2, &preimage)?;
    let (updates, _) = pipeline.build();
}

htlc_mgr.mark_fulfilled(&hash_lock)?;
```

## 7. On-Chain Operations

| Instruction | Function | Trigger Condition |
|:-----|:-----|:---------|
| `verify_htlc` | `build_verify_htlc_ix` | During Challenged or Settling phase, beneficiary provides preimage to claim |
| `htlc_refund` | `build_htlc_refund_ix` | During Challenged or Settling phase, refund after timelock expires |

## 8. Error Handling

| Error | Cause | Handling |
|:-----|:-----|:-----|
| `InvalidPreimage` | Preimage hash does not match hash_lock | Confirm the correct preimage |
| `HtlcNotExpired` | Timelock has not expired when attempting refund | Wait for more slots |
| `HtlcAlreadyResolved` | Attempting to resolve an already resolved HTLC | Check HTLC status |
| `TimelockConstraint` | Timelock does not satisfy design constraints | `timelock > current_slot + challenge_duration + HTLC_SAFETY_MARGIN` |

## 9. Notes

- HTLC lifecycle: `Pending → Revealed → Fulfilled` or `Pending → Expired → Refunded`
- The preimage must be kept confidential until service confirmation
- An HTLC leaf occupies one leaf slot; it is released after resolution
- Timelock constraint: `timelock_slot > current_slot + challenge_duration + HTLC_SAFETY_MARGIN` (1000 slots)
- All active HTLCs must be resolved before closing the channel (`close_channel` will check)

---

## Related Scenarios

| Scenario | Relationship |
|:-----|:-----|
| [01 Open Channel](01-channel-open.md) | Prerequisite: requires an open channel |
| [03 Batch Pipeline](03-batch-pipeline.md) | HTLC can be created within Pipeline |
| [05 Cooperative Close](05-cooperative-close.md) | HTLC must be resolved before closing |
| [07 HTLC Settlement](07-htlc-settlement.md) | On-chain HTLC claim/refund |
| [09 Multi-hop Payment](09-multihop-payment.md) | Multi-hop uses HTLCs with decreasing timelocks |
