# Scenario 10: Auto-Close and Watchtower

## 1. Scenario Description

Channels can be configured with an auto-close time (`auto_close_slot`). After it expires, any third party (Watchtower) can trigger settlement. This protects the funds of offline users and prevents counterparties from submitting stale states.

## 2. Participants

| Role | Responsibility |
|:-----|:-----|
| User | Set auto_close_slot when opening a channel |
| Watchtower | Monitor channels, trigger settlement on expiry (optional) |
| Provider | Participate in the normal settlement flow |

## 3. Prerequisites

- Channel is open
- `auto_close_offset` is configured (in config.toml) or `auto_close_slot` is specified at channel open time

## 4. Operation Flow

```
User                                   Watchtower                 Solana
 │  1. Set auto_close_slot               │                          │
 │  auto_close_slot = slot + offset       │                          │
 │                                        │                          │
 │  ... User goes offline ...             │                          │
 │                                        │                          │
 │                                        │  2. Monitor auto_close_slot
 │                                        │  Detect expiry            │
 │                                        │                          │
 │                                        │  3. POST /v1/channels/{id}/settle
 │                                        │──────────────────────────→│
 │                                        │  auto_settle(slot)       │
 │                                        │       Channel → Settling  │
 │                                        │                          │
 │  4. User comes online and claims leaves │                          │
 │  claim + finalize flow                 │                          │
```

## 5. HTTP API Calls

### Set Auto-Close When Opening Channel

In the open request, `auto_close_slot` is automatically calculated as `current_slot + auto_close_offset` (the value from the configuration file).

### Trigger Auto Settlement

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/settle \
  -H "Content-Type: application/json" \
  -d '{"settle_window": 10000}'
```

Watchtower can also call this endpoint (using the User or Provider service address).

## 6. Rust Library Calls

```rust
// Set auto_close_slot when opening
let state = mgr.open_channel(
    &user_pk, &provider_pk, &token_mint,
    1_000_000, 4, current_slot,
    &vault_a, &vault_b, 5000, 1000,
    Some(current_slot + 500_000),  // auto_close_slot
)?;

// Can also be set later
mgr.set_auto_close_slot(&mut state, Some(target_slot))?;

// Watchtower triggers settlement
mgr.auto_settle(&mut state, current_slot, settle_window)?;
```

## 7. On-Chain Operations

Auto settlement uses the `settle_after_timeout` instruction, which shares the same on-chain instruction as the dispute timeout settlement.

| Instruction | Function | Description |
|:-----|:-----|:-----|
| `settle_after_timeout` | `build_settle_after_timeout_ix` | Verifies auto_close_slot has passed |

## 8. Error Handling

| Error | Cause | Handling |
|:-----|:-----|:-----|
| `AutoCloseNotReached` | auto_close_slot has not expired | Wait until expiry before triggering |
| `ChannelNotOpen` | Channel is no longer in Open status | Check current status |
| `ActiveHtlcsExist` | Active HTLCs exist (informational) | Non-blocking error, auto_settle continues; HTLC funds must be handled separately via `verify_htlc` / `htlc_refund` within the settlement window (→ Scenario 07) |

## 9. Notes

- `auto_close_offset` is configured in config.toml in units of slots (500000 ≈ 55.6 hours)
- Setting to 0 disables auto-close
- auto_settle skips the challenge_duration wait and goes directly to Settling
- Watchtower can be any continuously running third-party service; it does not need to hold keys
- It is recommended that Users periodically come online to check channel status and promptly claim leaves

---

## Related Scenarios

| Scenario | Relationship |
|:-----|:-----|
| [01 Channel Open](01-channel-open.md) | Prerequisite: channel must have `auto_close_offset` configured |
| [05 Cooperative Close](05-cooperative-close.md) | After auto settlement, the claim + finalize flow is the same as cooperative close |
| [06 Dispute Resolution](06-dispute-resolution.md) | Uses the same `settle_after_timeout` on-chain instruction |
