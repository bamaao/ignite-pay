# Scenario 1: Channel Open & Funding

## 1. Scenario Description

A User and a Merchant/Provider establish a bidirectional payment channel. The user deposits initial funds, and a Merkle tree is created within the channel to manage UTXO balances. Optionally, the Provider can also fund the channel.

## 2. Participating Roles

| Role | Responsibility |
|:-----|:-----|
| User | Initiate channel, deposit initial funds, build split tree |
| Provider | Optional funding, co-sign initial state |

## 3. Prerequisites

- Both User and Provider have deployed the `channel-service` service
- Both parties hold Solana keypairs and SPL Token accounts
- The on-chain program (`ignite-pay-program`) has been deployed to the target cluster
- User knows the Provider's public key and Token account address

## 4. Operation Flow

```
User                                  Provider                                  Solana
 │                                       │                                       │
 │  1. POST /v1/channels/open            │                                       │
 │──────────────────────────────────────→│                                       │
 │  ChannelManager::open_channel         │                                       │
 │  (create Merkle tree, generate channel_id)                                    │
 │                                       │                                       │
 │  2. Build open_channel instruction     │                                       │
 │  build_open_channel_ix(...)           │                                       │
 │──────────────────────────────────────────────────────────────────────────────→│
 │                                       │                          Create PDA accounts    │
 │                                       │                          Deposit initial funds    │
 │  3. (Optional) POST /v1/channels/{id}/fund│                                    │
 │──────────────────────────────────────→│                                       │
 │                                       │  4. Provider funds                     │
 │                                       │  build_fund_channel_ix                │
 │                                       │──────────────────────────────────────→│
 │                                       │                                       │
 │  5. POST /v1/channels/{id}/split      │                                       │
 │  construct_split_tree(leaves)         │                                       │
 │──────────────────────────────────────→│                                       │
 │                                       │  6. Provider co-signs                 │
 │                                       │  provider_cosign_state                │
 │←──────────────────────────────────────│                                       │
```

## 5. HTTP API Calls

### Open Channel

```bash
curl -X POST http://localhost:3001/v1/channels/open \
  -H "Content-Type: application/json" \
  -d '{
    "provider_pubkey": "Provider Solana public key (Base58)",
    "token_mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    "deposit_amount": 1000000,
    "tree_depth": 4,
    "vault_a": "User SPL Token account",
    "vault_b": "Provider SPL Token account"
  }'
```

The response contains `channel_id` and on-chain instruction data for assembling a Solana transaction.

### Provider Funding

```bash
curl -X POST http://localhost:3002/v1/channels/{channel_id}/fund \
  -H "Content-Type: application/json" \
  -d '{
    "deposit_amount": 500000
  }'
```

### Build Split Tree

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/split \
  -H "Content-Type: application/json" \
  -d '{
    "leaves": [
      {"owner": "User public key", "amount": 500000},
      {"owner": "User public key", "amount": 200000},
      {"owner": "User public key", "amount": 300000}
    ]
  }'
```

## 6. Rust Library Calls

```rust
use ignite_pay_state_channel::channel::ChannelManager;
use ignite_pay_state_channel::signing::{generate_keypair, to_pubkey};

let db = sled::open("./data/user")?;
let mgr = ChannelManager::new(db)?;

// Open channel
let mut state = mgr.open_channel(
    &user_pubkey,
    &provider_pubkey,
    &token_mint,
    1_000_000,          // deposit_a
    4,                   // tree_depth (16 leaves)
    current_slot,
    &vault_a,
    &vault_b,
    5000,                // challenge_duration
    1000,                // min_challenge_delay
    None,                // auto_close_slot
)?;

// Provider funding
mgr.fund_channel(&mut state, &provider_kp, 500_000, None)?;

// Build split tree
let leaves = vec![
    UTXOLeaf::standard(user_pubkey, 500_000),
    UTXOLeaf::standard(user_pubkey, 200_000),
    UTXOLeaf::standard(user_pubkey, 300_000),
];
let signed = mgr.construct_split_tree(&mut state, leaves, &user_kp, &provider_kp)?;
```

## 7. On-chain Operations

| Instruction | Function | Description |
|:-----|:-----|:-----|
| `open_channel` | `build_open_channel_ix` | Create ChannelAccount PDA + Escrow PDA |
| `fund_channel` | `build_fund_channel_ix` | Provider injects funds into Escrow |

PDA derivation:
- Channel PDA: `seeds = ["channel", channel_id]`
- Escrow PDA: `seeds = ["escrow", channel_id]`

## 8. Error Handling

| Error | Cause | Resolution |
|:-----|:-----|:-----|
| `InvalidKeypair` | Keypair file format error | Check keypair_path configuration |
| `ChannelNotFound` | channel_id does not exist | Confirm channel has been opened |
| `AmountConservation` | Split tree amounts not conserved | Check that leaves amount sum = total_deposited |
| `SolanaRpc` | RPC call failed | Check rpc_url configuration and network connection |

## 9. Notes

- `tree_depth` range 3-12, corresponding to 8-4096 leaf slots, hardcoded limit in on-chain program
- Split tree requires amount conservation: sum of all leaf amounts must equal `total_deposited`
- `construct_split_tree` requires Provider's keypair for co-signing
- When `keypair_path` is empty, a temporary key is auto-generated (changes on each restart, for testing only)

---

## Related Scenarios

| Scenario | Relationship |
|:-----|:-----|
| [02 Off-chain Payment](02-offchain-payment.md) | Perform off-chain transfers after channel is open |
| [03 Batch Pipeline](03-batch-pipeline.md) | Batch operations after channel is open |
| [04 HTLC Payment](04-htlc-payment.md) | Create conditional payment after channel is open |
| [05 Cooperative Close](05-cooperative-close.md) | End of channel lifecycle |
