# Scenario 3: Batch Payment & Atomic Operations

## 1. Scenario Description

The user uses a Pipeline to execute atomic batch processing of multi-step operations. All operations within a Pipeline either all succeed or all roll back. This is suitable for complex scenarios that require simultaneously executing multiple payments, splits, and HTLC creations.

## 2. Participating Roles

| Role | Responsibility |
|:-----|:-----|
| User | Build Pipeline, execute batch operations |
| Provider | Accept and co-sign batch updates |

## 3. Prerequisites

- Channel is opened, status is `Open`
- Split tree is completed, with sufficient available leaf slots

## 4. Operation Flow

```
User
 │
 │  1. Create Pipeline
 │  Pipeline::new(&mut tree, channel_id, sequence, &keypair)
 │
 │  2. Execute operations (multiple can be stacked)
 │  ├─ transfer_leaf(0, provider_pubkey)         // Whole-leaf transfer
 │  ├─ partial_transfer(1, 4, 50000, provider)   // Partial transfer
 │  ├─ create_htlc(2, hash_lock, timelock, ...)   // Create HTLC
 │  └─ ...
 │
 │  3a. Success → pipeline.build()
 │  Returns Vec<SignedLeafUpdate>
 │
 │  3b. Failure → pipeline.abort()
 │  Tree state automatically restored to before Pipeline creation
 │
 │  4. Send signed updates to Provider
 │  POST /v1/channels/{id}/batch
 │→Provider
```

## 5. HTTP API Calls

### Batch Payment

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/batch \
  -H "Content-Type: application/json" \
  -d '{
    "updates": [
      {"leaf_index": 0, "new_owner": "Provider public key", "amount": 100000},
      {"leaf_index": 1, "new_owner": "Provider public key", "amount": 50000}
    ]
  }'
```

### Provider Accepts Batch

```bash
curl -X POST http://localhost:3002/v1/channels/{channel_id}/accept-batch \
  -H "Content-Type: application/json" \
  -d '{
    "updates": [...]
  }'
```

## 6. Rust Library Calls

```rust
use ignite_pay_state_channel::pipeline::Pipeline;

let mut tree = state.tree.clone();
{
    let mut pipeline = Pipeline::new(&mut tree, channel_id, sequence + 1, &user_keypair);

    // Whole-leaf transfer
    pipeline.transfer_leaf(0, provider_pubkey)?;

    // Partial transfer: split 50000 from leaf 1 into empty slot 4
    pipeline.partial_transfer(1, 4, 50_000, provider_pubkey)?;

    // Create HTLC
    pipeline.create_htlc(2, hash_lock, timelock_slot, provider_pubkey, current_slot, challenge_duration)?;

    // Commit: returns all signed LeafUpdates
    let (updates, final_sequence) = pipeline.build();

    // Send to Provider...
}
// If an error occurs midway, explicitly call pipeline.abort() or let Pipeline drop to auto-rollback
```

### Batch Application

```rust
// Provider batch accept
let result = mgr.apply_leaf_update_batch_with_info(
    &mut state,
    &updates,
    &user_pubkey,
);

match result {
    Ok(()) => { /* All succeeded */ },
    Err(info) => {
        // info.failed_index — index of the first failed update
        // info.error — failure reason
        // info.applied_count — number of successfully applied updates
    },
}
```

## 7. On-chain Operations

Pipeline operations are entirely off-chain and require no on-chain interaction.

## 8. Error Handling

| Error | Cause | Resolution |
|:-----|:-----|:-----|
| `BatchFailureInfo` | A mid-batch update failed | Check `failed_index` and `error` fields |
| `LeafNotEmpty` | Partial transfer target slot already occupied | Use an empty leaf index |
| `InsufficientAmount` | Source leaf has insufficient balance | Check leaf balance |
| `InvalidLeafState` | Whole-leaf transfer on an empty leaf | Confirm the leaf has a balance |

## 9. Notes

- Pipeline binds `&mut tree`; only one active Pipeline can exist at a time
- `partial_transfer` creates the destination leaf first, then deducts from the source leaf, ensuring amount conservation at each step
- `build()` consumes the Pipeline; no methods can be called afterward
- If a Pipeline is dropped without calling `build()` or `abort()`, the Drop trait automatically rolls back
- On batch failure, already-applied updates are not automatically rolled back (requires cooperative resolution or dispute resolution)

---

## Related Scenarios

| Scenario | Relationship |
|:-----|:-----|
| [01 Open Channel](01-channel-open.md) | Prerequisite: requires an opened channel |
| [02 Off-chain Payment](02-offchain-payment.md) | Pipeline executes transfer operations |
| [04 HTLC Payment](04-htlc-payment.md) | HTLC leaves can be created within Pipeline |
| [06 Dispute Resolution](06-dispute-resolution.md) | Dispute handling for partial batch failures |
