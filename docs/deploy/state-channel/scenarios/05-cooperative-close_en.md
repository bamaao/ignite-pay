# Scenario 5: Cooperative Channel Close

## 1. Scenario Description

The User and Provider both agree on the current channel state and jointly sign to close the channel. Funds are distributed according to the UTXO leaves held by each party, executed via the on-chain `cooperative_settle` instruction.

## 2. Participating Roles

| Role | Responsibility |
|:-----|:-----|
| User | Initiates the close request, provides their own signature |
| Provider | Co-signs to confirm, claims their own leaves |

## 3. Prerequisites

- Channel status is `Open`
- No active HTLCs in the channel (all HTLCs resolved or refunded)
- Both parties agree on the current Merkle root

## 4. Operation Flow

```
User                                   Provider                                Solana
 │  1. Request co-sign                    │                                       │
 │  POST /v1/channels/{id}/cosign         │                                       │
 │───────────────────────────────────────→│                                       │
 │                                        │  2. Provider co-signs                  │
 │←───────────────────────────────────────│  provider_cosign_state                │
 │  cosignature                           │                                       │
 │                                        │                                       │
 │  3. Build dual-signed state             │                                       │
 │  SignedState{sig_a, sig_b}             │                                       │
 │                                        │                                       │
 │  4. POST /v1/channels/{id}/close       │                                       │
 │  close_channel(signed_state, ...)      │                                       │
 │  build_cooperative_settle_ix           │                                       │
 │────────────────────────────────────────────────────────────────────────────────→│
 │                                        │                      Channel → Settling│
 │                                        │                                       │
 │  5. Claim leaves within settlement window│                                      │
 │  POST /v1/channels/{id}/claim          │                                       │
 │  claim_leaf_with_proof                 │                                       │
 │────────────────────────────────────────────────────────────────────────────────→│
 │                                        │  6. Provider also claims               │
 │                                        │──────────────────────────────────────→│
 │                                        │                                       │
 │  7. POST /v1/channels/{id}/finalize    │                                       │
 │  finalize_settlement                   │                                       │
 │────────────────────────────────────────────────────────────────────────────────→│
 │                                        │                      Channel Closed    │
```

## 5. HTTP API Calls

### Cooperative Close

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/close \
  -H "Content-Type: application/json" \
  -d '{"settle_window": 10000}'
```

Response:
```json
{
  "channel_id": "hex...",
  "status": "settling",
  "settle_window": 10000,
  "on_chain_instruction": {
    "program_id": "...",
    "data": "bs58-encoded instruction data"
  }
}
```

### Claim Leaf

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/claim \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 0,
    "claim_amount": 500000,
    "proof": ["hash1_hex", "hash2_hex", "hash3_hex", "hash4_hex"]
  }'
```

### Final Settlement

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/finalize
```

## 6. Rust Library Calls

```rust
// Build dual-signed state
let sig_a = sign_state(&channel_id, sequence, &root, &user_keypair);
let cosignature = mgr.provider_cosign_state(&mut state, &provider_keypair)?;

let signed_state = SignedState {
    channel_id,
    sequence: state.metadata.sequence,
    root: state.metadata.current_root,
    sig_a,
    sig_b: cosignature,
};

// Cooperative close
mgr.close_channel(&mut state, &signed_state, &user_pk, &provider_pk, current_slot, settle_window)?;

// Claim leaf
mgr.claim_leaf_with_proof(&mut state, leaf_index, claim_amount, &claimer_pk, current_slot, &claimer_sig, &proof)?;

// Final settlement
mgr.finalize_settlement(&mut state, current_slot, &caller_pk, &caller_sig)?;
```

## 7. On-Chain Operations

| Instruction | Function | Description |
|:-----|:-----|:-----|
| `cooperative_settle` | `build_cooperative_settle_ix` | Verifies dual signatures, enters Settling |
| `claim` | `build_claim_ix` | Claim standard leaf with Merkle Proof |
| `finalize_settlement` | `build_finalize_settlement_ix` | Distributes unclaimed funds, closes channel |

## 8. Error Handling

| Error | Cause | Handling |
|:-----|:-----|:-----|
| `ActiveHtlcsExist` | Channel still has unresolved HTLCs | Resolve or refund all HTLCs first |
| `InvalidSignature` | Dual signature verification failed | Confirm sig_a and sig_b are correct |
| `ProofVerificationFailed` | Merkle Proof is invalid | Regenerate the proof |
| `SettleWindowNotExpired` | Settlement window has not ended | Wait for more slots |

## 9. Notes

- `close_channel` will reject channels with active HTLCs
- `settle_window` determines the time window (in slots) for claiming leaves
- Unclaimed funds are distributed proportionally to `deposit_a/deposit_b` during `finalize_settlement`
- The on-chain `cooperative_settle` verifies that both signatures correspond to the channel's user_pubkey and provider_pubkey

---

## Related Scenarios

| Scenario | Relationship |
|:-----|:-----|
| [01 Open Channel](01-channel-open.md) | Prerequisite: requires an open channel |
| [02 Off-chain Payment](02-offchain-payment.md) | Prerequisite: channel should have payment activity |
| [04 HTLC Payment](04-htlc-payment.md) | Prerequisite: all active HTLCs must be resolved |
| [06 Dispute Resolution](06-dispute-resolution.md) | Alternative path when cooperation fails |
| [07 HTLC Settlement](07-htlc-settlement.md) | HTLC leaf claiming within settlement window |
| [10 Auto Close](10-auto-close.md) | Another channel closing method |
