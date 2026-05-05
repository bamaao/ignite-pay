# Scenario 7: HTLC Settlement and Refund

## 1. Scenario Description

During the dispute or settlement phase, on-chain claiming or refunding of HTLC leaves in the channel. The beneficiary can claim locked funds by revealing the preimage; if the timelock expires with no preimage revealed, the owner can reclaim the funds.

## 2. Participating Roles

| Role | Responsibility |
|:-----|:-----|
| Beneficiary | Holds the preimage, claims HTLC funds |
| Owner | Refunds after HTLC timeout |

## 3. Prerequisites

- Channel is in `Challenged` or `Settling` state
- HTLC leaf exists in the Merkle tree
- Claim: correct preimage available and `current_slot < timelock_slot`
- Refund: `current_slot > timelock_slot`

## 4. Operation Flow

### Case A — Beneficiary Claim

```
Beneficiary                              Solana
 │  1. Prepare parameters: leaf_index, preimage   │
 │     hash_lock, amount, beneficiary      │
 │     Merkle proof, claimer_signature     │
 │                                        │
 │  2. claim_htlc_verify                  │
 │───────────────────────────────────────→│
 │                                        │  Verify SHA-256(preimage) == hash_lock
 │                                        │  Verify current_slot < timelock_slot
 │                                        │  Verify Merkle proof
 │                                        │  Funds transferred to beneficiary
```

### Case B — Timeout Refund

```
Owner                                    Solana
 │  1. Confirm timelock has expired        │
 │     current_slot > timelock_slot        │
 │                                        │
 │  2. claim_htlc_refund                  │
 │───────────────────────────────────────→│
 │                                        │  Verify timelock expired
 │                                        │  Verify Merkle proof
 │                                        │  Funds returned to owner
```

## 5. HTTP API Calls

### HTLC Claim (via claim endpoint)

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/claim \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 2,
    "claim_amount": 100000,
    "proof": ["hash1_hex", "hash2_hex", ...]
  }'
```

### HTLC Refund

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/htlc/refund \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 2
  }'
```

## 6. Rust Library Calls

```rust
// Beneficiary claims HTLC
mgr.claim_htlc_verify(
    &mut state,
    leaf_index,
    &preimage,
    &claimer_pubkey,
    current_slot,
    &claimer_signature,
)?;

// Timeout refund
mgr.claim_htlc_refund(
    &mut state,
    leaf_index,
    &claimer_pubkey,
    current_slot,
    &claimer_signature,
)?;
```

## 7. On-Chain Operations

| Instruction | Function | Parameters |
|:-----|:-----|:-----|
| `verify_htlc` | `build_verify_htlc_ix` | leaf_index, preimage, hash_lock, amount, beneficiary, leaf_hash, timelock_slot, leaf_data, proof[], claimer_sig |
| `htlc_refund` | `build_htlc_refund_ix` | leaf_index, hash_lock, amount, owner, leaf_hash, timelock_slot, leaf_data, proof[], claimer_sig |

## 8. Error Handling

| Error | Cause | Handling |
|:-----|:-----|:-----|
| `InvalidPreimage` | Preimage hash does not match hash_lock | Provide the correct preimage |
| `HtlcNotExpired` | Timelock has not expired when attempting refund | Wait for more slots |
| `HtlcExpired` | Timelock has expired when attempting claim | Cannot claim, use refund instead |
| `ProofVerificationFailed` | Merkle proof is invalid | Regenerate proof from the current tree |
| `NotBeneficiary` | Claimer is not the beneficiary | Use the beneficiary's signature |

## 9. Notes

- HTLC claiming and refunding can only be executed in `Challenged` or `Settling` state
- In `Challenged` state, `settle_deadline` is not yet set; the on-chain deadline for operations is `challenge_slot + challenge_duration`
- In `Settling` state, `settle_deadline` is used as the operation deadline
- `verify_htlc` requires 11 parameters, making it the most complex on-chain instruction
- Merkle proof must be generated based on the current `current_root`
- Claimer signature message format (on-chain): `channel_id || current_slot || current_root`
- For refund, the on-chain program verifies `timelock_slot < current_slot`

---

## Related Scenarios

| Scenario | Relationship |
|:-----|:-----|
| [04 HTLC Payment](04-htlc-payment.md) | Prerequisite: off-chain HTLC creation and preimage management |
| [05 Cooperative Close](05-cooperative-close.md) | Claim HTLC leaves within settlement window |
| [06 Dispute Resolution](06-dispute-resolution.md) | HTLC claiming deadline during Challenged/Settling state |
| [09 Multi-hop Payment](09-multihop-payment.md) | On-chain resolution of HTLC at each hop |
