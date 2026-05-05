# Scenario 6: Dispute Resolution

## 1. Scenario Description

When one party is unresponsive or submits an outdated state, the other party can initiate a dispute (challenge) on-chain. During the dispute period, the counterparty may submit an updated counter-state. If no response is received before timeout, the channel proceeds directly to settlement.

## 2. Participating Roles

| Role | Responsibility |
|:-----|:-----|
| Challenger | Submits the dispute, provides signed state |
| Counterparty | Submits a counter-state during the dispute period (optional) |

## 3. Prerequisites

- Channel status is `Open`
- At least `min_challenge_delay` slots have passed since the last state update
- Challenger holds a valid signed state (with a sequence higher than the current on-chain record)

## 4. Operation Flow

```
Challenger                             Counterparty                    Solana
 │                                        │                              │
 │  1. Sign dispute message                │                              │
 │  sign(channel_id || slot || root)      │                              │
 │                                        │                              │
 │  2. POST /v1/channels/{id}/challenge   │                              │
 │────────────────────────────────────────────────────────────────────────→│
 │  trigger_challenge                     │              Channel → Challenged│
 │  build_trigger_challenge_ix            │                              │
 │                                        │                              │
 │       === challenge_duration countdown ===                            │
 │                                        │                              │
 │                                        │  3a. (Optional) Submit counter-state│
 │                                        │  submit_counter_state         │
 │                                        │  build_submit_counter_state_ix│
 │                                        │─────────────────────────────→│
 │                                        │              Verify sig_a+sig_b│
 │                                        │                              │
 │  3b. Timeout with no counter-state     │                              │
 │  POST /v1/channels/{id}/settle         │                              │
 │  settle_after_timeout                  │                              │
 │────────────────────────────────────────────────────────────────────────→│
 │                                        │              Channel → Settling│
 │                                        │                              │
 │  4. Normal claim + finalize flow       │                              │
```

## 5. HTTP API Calls

### Initiate Dispute

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/challenge \
  -H "Content-Type: application/json" \
  -d '{
    "submitted_root": "hex-encoded 32-byte Merkle root",
    "submitted_sequence": 5
  }'
```

### Submit Counter-State

```bash
curl -X POST http://localhost:3002/v1/channels/{channel_id}/submit-counter \
  -H "Content-Type: application/json" \
  -d '{
    "sig_a": "hex-encoded 64-byte signature A",
    "sig_b": "hex-encoded 64-byte signature B"
  }'
```

### Timeout Settlement

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/settle \
  -H "Content-Type: application/json" \
  -d '{"settle_window": 10000}'
```

## 6. Rust Library Calls

```rust
// Initiate dispute
mgr.trigger_challenge(
    &mut state,
    &challenger_pubkey,
    current_slot,
    &submitted_root,
    submitted_sequence,
    &challenger_signature,
)?;

// Submit counter-state
let counter_state = SignedState { channel_id, sequence: higher_seq, root, sig_a, sig_b };
mgr.submit_counter_state(&mut state, &counter_state, None, &user_pk, &provider_pk)?;

// Settle after timeout
mgr.settle_after_timeout(&mut state, current_slot, settle_window)?;
```

## 7. On-Chain Operations

| Instruction | Function | Description |
|:-----|:-----|:-----|
| `trigger_challenge` | `build_trigger_challenge_ix` | Records the dispute slot and submitted root |
| `submit_counter_state` | `build_submit_counter_state_ix` | Verifies a dual-signed state with a higher sequence |
| `settle_after_timeout` | `build_settle_after_timeout_ix` | Enters Settling after challenge_duration expires |

## 8. Error Handling

| Error | Cause | Handling |
|:-----|:-----|:-----|
| `ChallengeTooEarly` | min_challenge_delay has not passed since last update | Wait for more slots |
| `InvalidSequence` | submitted_sequence is not higher than current | Submit a state with a higher sequence |
| `InvalidSignature` | Signature verification failed | Confirm signature corresponds to the correct public key |
| `NoActiveChallenge` | settle_after_timeout called but no dispute exists | Trigger a challenge first |
| `CounterStateExpired` | Counter-state submitted after challenge_duration | Already entered timeout settlement |

## 9. Notes

- Dispute signature message format: `channel_id || current_slot || submitted_root`
- `min_challenge_delay` prevents front-running attacks (cannot initiate dispute too early)
- `submit_counter_state` requires both sig_a + sig_b signatures, proving both parties agreed to that state
- After timeout settlement, the flow is the same as cooperative close: claim leaves → finalize
- During the `Challenged` state (within `challenge_duration` countdown), HTLC claim or refund operations can also be executed (deadline is `challenge_slot + challenge_duration`)
- Challenger's signature is generated using `ed_kp.sign(msg)`, and the message includes slot to prevent replay

---

## Related Scenarios

| Scenario | Relationship |
|:-----|:-----|
| [01 Open Channel](01-channel-open.md) | Prerequisite: requires an open channel |
| [04 HTLC Payment](04-htlc-payment.md) | HTLC operations during Challenged state |
| [05 Cooperative Close](05-cooperative-close.md) | After dispute timeout, flow is the same as cooperative close (claim + finalize) |
| [07 HTLC Settlement](07-htlc-settlement.md) | On-chain HTLC settlement within dispute window |
| [10 Auto Close](10-auto-close.md) | Uses the same `settle_after_timeout` on-chain instruction |
