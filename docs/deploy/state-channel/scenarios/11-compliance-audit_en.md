# Scenario 11: Compliance Management and Audit

## 1. Scenario Description

Channels can enable compliance management features to monitor payment activity in real time. When cumulative payments exceed a threshold, a compliance review is automatically triggered (by inserting a compliance marker leaf and freezing the channel), while maintaining a complete audit trail.

## 2. Participants

| Role | Responsibility |
|:-----|:-----|
| User/Provider | Configure compliance parameters, record payments and audits |
| ComplianceManager | Perform limit checks, generate compliance leaves |

## 3. Prerequisites

- config.toml contains the `[compliance]` configuration section
- Compliance state is initialized after channel opening

## 4. Operation Flow

```
User                                ComplianceManager
 │  1. Initialize compliance             │
 │  init_channel_compliance(channel_id, │
 │    SpendingLimit{threshold,           │
 │    per_channel, window_slots})        │
 │─────────────────────────────────────→│
 │                                      │
 │  2. Record after each payment         │
 │  record_payment(channel_id, amount,  │
 │    slot, user_pk, provider_pk)        │
 │─────────────────────────────────────→│
 │                                      │  3. Check sliding window
 │                                      │  Cumulative payments > threshold?
 │                                      │
 │  ← ComplianceAction::None            │  (Normal, no action)
 │  Or                                  │
 │  ← ComplianceAction::InsertMarker    │  (Trigger compliance review)
 │     {compliance_hash, threshold}     │
 │                                      │
 │  4. (If triggered) Create compliance leaf
 │  create_compliance_leaf(...)         │
 │  → Insert into Merkle tree           │
 │                                      │
 │  5. (If triggered) Channel frozen     │
 │  Subsequent payments are blocked      │
 │                                      │
 │  6. After compliance review passes     │
 │  clear_hold(channel_id)              │
 │─────────────────────────────────────→│
 │  Channel returns to normal            │
```

### Audit Trail

```
After each LeafUpdate:
  record_audit(&leaf_update)
  → Store: {sequence, leaf_index, new_leaf, timestamp}

Query:
  get_audit_trail(channel_id)
  → Vec<AuditEntry> (sorted by sequence)
```

## 5. HTTP API Calls

### Query Compliance Status

```bash
curl http://localhost:3001/v1/compliance/{channel_id}
```

Response example:
```json
{
  "channel_id": "hex...",
  "total_spent": 500000000,
  "threshold": 1000000000,
  "window_spent": 200000000,
  "window_slots": 100000,
  "hold_active": false
}
```

### Compliance Check on Payment

The payment endpoint (`/v1/channels/{id}/pay`) internally calls `record_payment` and checks compliance automatically. If a hold is triggered, it returns an error:

```json
{
  "error": "ComplianceHold",
  "message": "Spending threshold exceeded, compliance review required"
}
```

## 6. Rust Library Calls

```rust
use ignite_pay_state_channel::compliance::{ComplianceManager, SpendingLimit};

let compliance = ComplianceManager::new(db.clone())?;

// Initialize channel compliance
compliance.init_channel_compliance(channel_id, SpendingLimit {
    threshold: 1_000_000_000,   // Cumulative spending threshold
    per_channel: 100_000_000,   // Max single-channel payment
    window_slots: 100_000,      // Sliding window
})?;

// Record after each payment
let action = compliance.record_payment(
    channel_id,
    payment_amount,
    current_slot,
    user_pubkey,
    provider_pubkey,
)?;

match action {
    ComplianceAction::None => { /* Normal */ },
    ComplianceAction::InsertMarker { compliance_hash, threshold } => {
        // Create compliance leaf and insert into Merkle tree
        let leaf = ComplianceManager::create_compliance_leaf(compliance_hash, threshold);
        // Channel enters hold state
    },
}

// Clear hold
compliance.clear_hold(channel_id)?;

// Audit trail
compliance.record_audit(&leaf_update)?;
let trail = compliance.get_audit_trail(channel_id)?;
```

## 7. On-Chain Operations

Compliance management is entirely off-chain. After a compliance leaf is inserted into the Merkle tree, its existence is reflected in the Merkle proof at settlement time.

## 8. Error Handling

| Error | Cause | Handling |
|:-----|:-----|:-----|
| `ComplianceHold` | Cumulative payments exceed threshold, channel frozen | Wait for compliance review to clear_hold |
| `PerChannelExceeded` | Single payment exceeds per_channel limit | Split into multiple smaller payments |
| `WindowExceeded` | Payments within sliding window exceed limit | Wait for window to roll forward |

## 9. Notes

- The `[compliance]` configuration section is optional; omitting it disables compliance features
- Compliance only applies to User and Hub roles (Provider does not configure compliance)
- The sliding window is based on Solana slots, not time
- `travel_rule_threshold` is used to flag large payments requiring Travel Rule reporting
- Audit records are append-only and cannot be deleted or modified
- `record_audit` should be called after each `apply_leaf_update`

---

## Related Scenarios

| Scenario | Relationship |
|:-----|:-----|
| [01 Channel Open](01-channel-open.md) | Prerequisite: must open channel and configure compliance parameters |
| [02 Off-chain Payment](02-offchain-payment.md) | Payments may trigger `ComplianceHold` error |
| [04 HTLC Payment](04-htlc-payment.md) | HTLC amounts are also subject to compliance limits |
