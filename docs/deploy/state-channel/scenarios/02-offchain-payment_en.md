# Scenario 2: Off-chain Payment & Split

## 1. Scenario Description

The user initiates an off-chain payment to the Provider within an opened channel. Two methods are supported: whole-leaf transfer (transferring entire UTXO leaf ownership to the Provider) and partial transfer (splitting a portion of the amount from a leaf into a new slot, then transferring it to the Provider). Each payment is verified via Ed25519 signature and takes effect after the Provider co-signs to confirm.

## 2. Participating Roles

| Role | Responsibility |
|:-----|:-----|
| User | Build signed LeafUpdate, send payment request |
| Provider | Verify signature, apply update, co-sign to confirm |

## 3. Prerequisites

- Channel is opened, status is `Open`
- Split tree is completed (multiple usable UTXO leaves available)
- User knows the leaf index and amount to pay

## 4. Operation Flow

```
User                                   Provider
 │                                        │
 │  1. Build LeafUpdate                   │
 │  sign_leaf_update(channel_id, seq,     │
 │    leaf_index, prev_leaf, new_leaf)    │
 │                                        │
 │  2. POST /v1/channels/{id}/pay         │
 │───────────────────────────────────────→│
 │                                        │  3. Verify user signature
 │                                        │  apply_leaf_update(state, update, &user_pk)
 │                                        │
 │  4. POST /v1/channels/{id}/cosign      │
 │───────────────────────────────────────→│
 │                                        │  5. Provider co-signs
 │                                        │  provider_cosign_state(state, &provider_kp)
 │←───────────────────────────────────────│
 │  cosignature                           │
```

## 5. HTTP API Calls

### Single Payment

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/pay \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 0,
    "new_owner": "Provider public key (Base58)",
    "amount": 100000
  }'
```

### Request Provider Co-signature

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/cosign \
  -H "Content-Type: application/json" \
  -d '{}'
```

### Provider Accepts Payment

```bash
curl -X POST http://localhost:3002/v1/channels/{channel_id}/accept-payment \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 0,
    "new_owner": "Provider public key",
    "amount": 100000,
    "sequence": 3,
    "signature": "signature hex..."
  }'
```

## 6. Rust Library Calls

```rust
use ignite_pay_state_channel::signing::sign_leaf_update;

// User signs LeafUpdate
let sig = sign_leaf_update(
    &channel_id,
    state.metadata.sequence + 1,
    leaf_index,
    &prev_leaf,
    &new_leaf,
    &user_keypair,
);

// Provider verifies and applies
mgr.apply_leaf_update(&mut state, &leaf_update, &user_pubkey)?;

// Provider co-signs
let cosignature = mgr.provider_cosign_state(&mut state, &provider_keypair)?;
```

## 7. On-chain Operations

Off-chain payments require no on-chain operations. All changes are persisted only in both parties' sled databases.

## 8. Error Handling

| Error | Cause | Resolution |
|:-----|:-----|:-----|
| `InvalidSignature` | Signature verification failed | Check signer public key and signature data |
| `SequenceMismatch` | Sequence number is not consecutive | Use current sequence + 1 |
| `LeafNotFound` | Leaf index out of range | Check tree_depth limit |
| `AmountConservation` | Total amount inconsistent after payment | Ensure split amount is correct for partial transfers |
| `ComplianceHold` | Under compliance review, payment blocked | Contact compliance admin to clear hold |

## 9. Notes

- Signature message format: `SHA-256(channel_id || sequence || leaf_index || prev_leaf_hash || new_leaf_hash)`
- Each payment increments sequence by 1; cannot skip or roll back
- Provider co-signature indicates agreement with the current state, which can later be used for cooperative close
- Partial transfers consume an empty leaf slot; be mindful of `tree_depth` limits

---

## Related Scenarios

| Scenario | Relationship |
|:-----|:-----|
| [01 Open Channel](01-channel-open.md) | Prerequisite: requires an opened channel |
| [03 Batch Pipeline](03-batch-pipeline.md) | Execute multiple transfers in batch |
| [04 HTLC Payment](04-htlc-payment.md) | Conditional payment (requires understanding basic transfers first) |
| [11 Compliance Audit](11-compliance-audit.md) | Transfers may trigger `ComplianceHold` |
