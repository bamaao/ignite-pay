# State Channel User-Side Deployment Configuration Guide

## 1. Overview

The user side (Party A, the payer) is the initiator of the state channel. Users manage the channel lifecycle through the `ignite-pay-state-channel` off-chain library, including opening channels, splitting UTXOs, signing payments, managing HTLC, and settlement.

There are two integration modes for the user side:

1. **Library Integration**: Embed `ignite-pay-state-channel` as a Rust library into the client application
2. **Service Deployment**: Run the `channel-user` binary from `ignite-pay-channel-service`, operating via HTTP REST + WebSocket interfaces

---

## 2. Core Components

| Component | crate | Description |
|:----------|:------|:------------|
| Channel Management | `ignite-pay-state-channel` | `ChannelManager` — channel open/close, state persistence |
| Merkle Tree | `ignite-pay-state-channel` | `MerkleTree` — binary Merkle tree for UTXO leaf nodes |
| Signing Module | `ignite-pay-state-channel` | `signing` — Ed25519 signing/verification |
| Pipeline | `ignite-pay-state-channel` | `Pipeline` — batch LeafUpdate construction |
| HTLC Management | `ignite-pay-state-channel` | `HtlcManager` — preimage generation/reveal/expiry |
| Compliance Module | `ignite-pay-state-channel` | `ComplianceManager` — spending limits/audit |
| On-Chain Instructions | `ignite-pay-solana` | `channel` — 10 on-chain Instruction builders |
| HTTP Service | `ignite-pay-channel-service` | REST + WebSocket service for the User role |

---

## 3. Option 1: Service Deployment (Recommended)

### 3.1 Build

```bash
cd ignite-pay-channel-service
cargo build --release --bin channel-user
```

The output binary is located at `target/release/channel-user` (Windows: `target/release/channel-user.exe`).

### 3.2 Generate Keys

```bash
# Generate a key file using the Solana CLI (64 bytes, JSON array format)
solana-keygen new --outfile ./keys/user.key

# Alternatively, use any Ed25519 keypair, saved as a 64-byte raw file (first 32 bytes private key + last 32 bytes public key)
```

> If `keypair_path` is left empty (`""`), the service will automatically generate a temporary keypair on startup (this changes on every restart, suitable only for testing).

### 3.3 Configuration File

Create `config.toml`:

```toml
[server]
host = "0.0.0.0"        # Listen address; for production, recommend "127.0.0.1" + reverse proxy
port = 3001              # Listen port

[solana]
rpc_url = "https://api.devnet.solana.com"          # Solana RPC endpoint
channel_program_id = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe"  # On-chain program ID
keypair_path = "./keys/user.key"                    # Ed25519 keypair file

[channel]
default_tree_depth = 4           # Default Merkle tree depth (2^4 = 16 leaves)
default_challenge_duration = 5000   # Default dispute period (slots, approximately 33 minutes)
default_min_challenge_delay = 1000  # Minimum dispute delay (slots)
default_settle_window = 10000       # Default settlement window (slots)
auto_close_offset = 500000          # Auto-close offset (slots, 0 means no auto-close)
db_path = "./data/channel_user"     # sled database path

# Optional: compliance configuration
[compliance]
spending_threshold = 1000000000     # Cumulative spending threshold (smallest unit)
per_channel_limit = 100000000       # Maximum payment per channel
window_slots = 100000               # Sliding window (slots)
travel_rule_threshold = 500000000   # Travel Rule trigger amount
```

### 3.4 Start the Service

```bash
# Use the default configuration file config.toml
./channel-user

# Specify a configuration file
./channel-user /path/to/config.toml

# Enable debug logging
RUST_LOG=debug ./channel-user
```

Log level is controlled via the `RUST_LOG` environment variable, supporting `trace` / `debug` / `info` / `warn` / `error`.

### 3.5 API Endpoints

The User role registers the following REST endpoints:

| Method | Path | Description | Off-Chain API | On-Chain Instruction |
|:-------|:-----|:------------|:--------------|:---------------------|
| GET | `/health` | Health check | — | — |
| POST | `/v1/channels/open` | Open channel | `ChannelManager::open_channel` | `build_open_channel_ix` |
| POST | `/v1/channels/{id}/fund` | Fund channel | — | `build_fund_channel_ix` |
| GET | `/v1/channels` | List channels | `list_channel_ids` | — |
| GET | `/v1/channels/{id}` | Query channel state | `load_state` | — |
| POST | `/v1/channels/{id}/split` | Build split tree | `construct_split_tree` | — |
| POST | `/v1/channels/{id}/pay` | Single payment | `apply_leaf_update` | — |
| POST | `/v1/channels/{id}/batch` | Batch payment | `apply_leaf_update_batch_with_info` | — |
| POST | `/v1/channels/{id}/cosign` | Request co-signing | `provider_cosign_state` | — |
| POST | `/v1/channels/{id}/close` | Cooperative close | `close_channel` | `build_cooperative_settle_ix` |
| POST | `/v1/channels/{id}/challenge` | Initiate dispute | `trigger_challenge` | `build_trigger_challenge_ix` |
| POST | `/v1/channels/{id}/settle` | Timeout settlement | `settle_after_timeout` | `build_settle_after_timeout_ix` |
| POST | `/v1/channels/{id}/claim` | Claim leaf | `claim_leaf_with_proof` | — |
| POST | `/v1/channels/{id}/finalize` | Final settlement | `finalize_settlement` | `build_finalize_settlement_ix` |
| POST | `/v1/channels/{id}/htlc/create` | Create HTLC | `HtlcManager::create_htlc` | — |
| POST | `/v1/channels/{id}/htlc/resolve` | Resolve HTLC | `reveal_preimage` | `build_verify_htlc_ix` |
| POST | `/v1/channels/{id}/htlc/refund` | HTLC refund | `claim_htlc_refund` | `build_htlc_refund_ix` |
| GET | `/v1/routes` | Query routes | `RouteService::find_routes` | — |
| POST | `/v1/multihop/create` | Create multi-hop payment | `MultiHopManager::create_payment` | — |
| POST | `/v1/multihop/{id}/resolve` | Resolve multi-hop | `resolve_hop` | — |
| GET | `/v1/compliance/{id}` | Compliance status | `ComplianceManager` | — |
| WS | `/ws` | WebSocket connection | — | — |

### 3.6 Example Requests

```bash
# Health check
curl http://localhost:3001/health

# Open channel
curl -X POST http://localhost:3001/v1/channels/open \
  -H "Content-Type: application/json" \
  -d '{
    "user_pubkey": "11111111111111111111111111111111",
    "provider_pubkey": "22222222222222222222222222222222",
    "token_mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    "deposit_amount": 1000000,
    "tree_depth": 4,
    "vault_a": "...",
    "vault_b": "..."
  }'

# List channels
curl http://localhost:3001/v1/channels

# Payment
curl -X POST http://localhost:3001/v1/channels/{channel_id}/pay \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 0,
    "new_owner": "22222222222222222222222222222222",
    "amount": 100000
  }'
```

### 3.7 WebSocket Protocol

Connect to `ws://localhost:3001/ws`, using a tagged JSON message format:

**Authentication**:

```json
 {"type": "auth", "pubkey": "<base58>", "signature": [64 bytes], "timestamp": 1234567890}
← {"type": "auth_ok"}
```

Signed content: `SHA-256("channel-ws-auth:{timestamp}")`

**Real-time LeafUpdate Push**:

```json
 {"type": "leaf_update", "channel_id": "hex", "sequence": 1, "leaf_index": 0,
   "prev_leaf_hash": [32 bytes], "new_leaf": {...}, "signature": [64 bytes]}
← {"type": "leaf_update_ack", "channel_id": "hex", "sequence": 2}
```

### 3.8 systemd Service (Linux Production Deployment)

Create `/etc/systemd/system/ignite-channel-user.service`:

```ini
[Unit]
Description=Ignite Pay Channel User Service
After=network.target

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/channel-user /opt/ignite-pay/config.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable ignite-channel-user
sudo systemctl start ignite-channel-user
sudo journalctl -u ignite-channel-user -f   # View logs
```

### 3.9 Nginx Reverse Proxy

```nginx
server {
    listen 443 ssl;
    server_name channel-user.ignite-pay.example.com;

    ssl_certificate     /etc/ssl/certs/ignite-pay.pem;
    ssl_certificate_key /etc/ssl/private/ignite-pay.key;

    location / {
        proxy_pass http://127.0.0.1:3001;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }

    location /ws {
        proxy_pass http://127.0.0.1:3001;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
    }
}
```

---

## 4. Option 2: Library Integration

### 4.1 Add Dependencies

In your Rust project's `Cargo.toml`:

```toml
[dependencies]
ignite-pay-state-channel = { path = "../ignite-pay-state-channel" }
solana-pubkey = "2"
solana-program = "2"
ed25519-dalek = "1"
```

### 4.2 Initialize ChannelManager

```rust
use ignite_pay_state_channel::channel::ChannelManager;
use ignite_pay_state_channel::signing::{generate_keypair, to_pubkey};
use solana_pubkey::Pubkey;

// Open sled database (all channel state is persisted here)
let db = sled::open("./user_channel_data")?;
let manager = ChannelManager::new(db)?;

// Generate or load user keypair
let user_keypair = generate_keypair();
let user_pubkey = to_pubkey(&user_keypair);
```

### 4.3 Open Channel

```rust
use ignite_pay_state_channel::channel::ChannelManager;

let provider_pubkey = Pubkey::new_from_array(/* Merchant/Provider public key */);
let token_mint = Pubkey::new_from_array(/* SPL Token Mint address, e.g. USDC */);
let vault_a = Pubkey::new_from_array(/* User SPL Token account */);
let vault_b = Pubkey::new_from_array(/* Provider SPL Token account */);

let state = manager.open_channel(
    &user_pubkey,           // User public key
    &provider_pubkey,       // Provider public key
    &token_mint,            // Token Mint
    1_000_000,              // Deposit amount (smallest unit)
    3,                      // tree_depth (2^3 = 8 leaf slots)
    current_slot,           // Opening slot
    &vault_a,               // User vault
    &vault_b,               // Provider vault
    500,                    // challenge_duration (slots)
    50,                     // min_challenge_delay (slots)
    None,                   // auto_close_slot (optional)
)?;

println!("Channel opened: channel_id = {}", hex::encode(state.metadata.channel_id));
println!("Initial root: {}", hex::encode(state.metadata.current_root));
```

**On-chain operation**: After opening the channel, you need to call the on-chain `open_channel` instruction to commit the channel state to Solana.

### 4.4 Build Split Tree

Split the initial deposit into UTXO leaves of various denominations:

```rust
use ignite_pay_state_channel::types::UTXOLeaf;

let leaves = vec![
    UTXOLeaf::standard(user_pubkey, 100_000),  // 100K
    UTXOLeaf::standard(user_pubkey, 200_000),  // 200K
    UTXOLeaf::standard(user_pubkey, 500_000),  // 500K
    UTXOLeaf::standard(user_pubkey, 200_000),  // 200K
    // Remaining slots are automatically filled with UTXOLeaf::empty()
];

let signed_state = manager.construct_split_tree(
    &mut state,
    leaves,
    &user_keypair,
    &provider_keypair,   // Requires Provider co-signing
)?;
```

**Note**: `construct_split_tree` requires amount conservation — the sum of all leaf amounts must equal `total_deposited`.

### 4.5 Execute Payments Using Pipeline

```rust
use ignite_pay_state_channel::pipeline::Pipeline;

let channel_id = state.metadata.channel_id;
let sequence = state.metadata.sequence;

let mut tree = state.tree;
{
    let mut pipeline = Pipeline::new(&mut tree, channel_id, sequence + 1, &user_keypair);

    // Whole-leaf transfer: transfer leaf 0 to the Provider
    pipeline.transfer_leaf(0, provider_pubkey)?;

    // Partial transfer: split 50_000 from leaf 1 into empty slot 4
    pipeline.partial_transfer(1, 4, 50_000, provider_pubkey)?;

    // Commit the pipeline
    let (updates, final_sequence) = pipeline.build();

    // updates contains all signed LeafUpdates
    // Send to the Provider for co-signing
}
```

**Pipeline Safety Mechanisms**:
- If an operation fails, call `pipeline.abort()` to roll back the tree state
- If the Pipeline is dropped without calling `build()` or `abort()`, it automatically rolls back

### 4.6 HTLC Payments

```rust
use ignite_pay_state_channel::htlc::HtlcManager;

let mut htlc_mgr = HtlcManager::with_db(db.clone(), channel_id);

// Create HTLC (generates a random preimage)
let (hash_lock, preimage) = htlc_mgr.create_htlc(
    100_000,           // Locked amount
    2,                 // Leaf index
    user_pubkey,       // Owner
    provider_pubkey,   // Beneficiary
    current_slot,      // Current slot
    500,               // Duration in slots
);

// Share the hash_lock with the Provider (do not reveal the preimage yet)
// The Provider can use the hash_lock to verify the HTLC leaf

// Create an HTLC leaf in the Pipeline
{
    let mut pipeline = Pipeline::new(&mut tree, channel_id, seq, &user_keypair);
    pipeline.create_htlc(
        2,                  // Leaf index
        hash_lock,
        timelock_slot,
        provider_pubkey,    // beneficiary
        current_slot,
        challenge_duration,
    )?;
    let (updates, _) = pipeline.build();
}

// After service is fulfilled, the Provider reveals the preimage
htlc_mgr.reveal_preimage(&hash_lock, &preimage)?;

// Resolve the HTLC in the Pipeline
{
    let mut pipeline = Pipeline::new(&mut tree, channel_id, seq, &user_keypair);
    pipeline.resolve_htlc(2, &preimage)?;
    let (updates, _) = pipeline.build();
}

htlc_mgr.mark_fulfilled(&hash_lock)?;
```

---

## 5. Channel Parameter Configuration

### 5.1 tree_depth Selection

| tree_depth | Maximum Leaves | Use Case |
|:-----------|:---------------|:---------|
| 3 | 8 | Small trial / single payment |
| 4 | 16 | Everyday payments |
| 5 | 32 | Medium-frequency transactions |
| 6 | 64 | High-frequency micropayments |
| 7 | 128 | Large number of concurrent HTLCs |
| 8 | 256 | Production-grade high-frequency transactions |
| 10 | 1024 | Long sessions / ultra-high concurrency |
| 12 | 4096 | Maximum throughput (on-chain maximum limit) |

> The on-chain program restricts `tree_depth <= 12`.

### 5.2 challenge_duration Selection

| Value (slots) | Approximate | Use Case |
|:--------------|:------------|:---------|
| 150 | ~1 minute | Testing environment |
| 500 | ~3.3 minutes | Small-value channels |
| 1500 | ~10 minutes | Standard |
| 4500 | ~30 minutes | Large-value channels |
| 9000 | ~1 hour | High-value dispute window |

### 5.3 Split Denomination Recommendations

Using a deposit of 1,000,000 units as an example:

```
tree_depth = 4 (16 slots):
  [500K, 200K, 100K, 50K, 50K, 50K, 50K, ...empty]
  Suitable for: medium-frequency payments

tree_depth = 5 (32 slots):
  [500K, 100K, 100K, 50K, 50K, 20K, 20K, 20K, 20K, 20K, 10K×10, ...empty]
  Suitable for: high-frequency micropayments + HTLC reservations
```

---

## 6. Data Persistence

### 6.1 sled Database

`ChannelManager` uses the sled embedded database to store all channel state:

| Storage Path | Content |
|:-------------|:--------|
| Database root directory | Channel metadata (`ChannelMetadata`), Merkle tree |
| `htlc:{channel_id}` | HTLC records |
| `compliance:{channel_id}` | Compliance status |
| `audit:{channel_id}:{seq}` | Audit trail |

### 6.2 Backup Recommendations

```bash
# sled data directory
./data/channel_user/

# Backup (ensure the process is stopped or use a snapshot)
cp -r ./data/channel_user/ ./data/channel_user_backup/
```

> sled data is automatically persisted to disk. After a restart, it can be restored via `ChannelManager::new(sled::open(path))`.

---

## 7. Settlement Operations

### 7.1 Cooperative Close (Recommended)

Both parties agree on the current state and jointly sign to close:

```bash
# Via HTTP API
curl -X POST http://localhost:3001/v1/channels/{channel_id}/close \
  -H "Content-Type: application/json" \
  -d '{"settle_window": 10000}'
```

```rust
// Via library call
let sig_a = sign_state(&channel_id, sequence, &root, &user_keypair);
let sig_b = sign_state(&channel_id, sequence, &root, &provider_keypair);
// Call on-chain cooperative_settle
```

### 7.2 Dispute Close

If the other party is unresponsive:

```bash
# Initiate a dispute via HTTP API
curl -X POST http://localhost:3001/v1/channels/{channel_id}/challenge \
  -H "Content-Type: application/json" \
  -d '{"submitted_root": "hex...", "submitted_sequence": 5}'
```

### 7.3 Auto-Close

If the channel has `auto_close_offset` configured:

```rust
let state = manager.open_channel(
    // ...
    Some(current_slot + 100_000),  // auto_close_slot
)?;

// After expiry, anyone can trigger settlement
manager.auto_settle(&mut state, settle_window)?;
```

---

## 8. Configuration Parameter Reference

| Parameter | Type | Default | Description |
|:----------|:-----|:--------|:------------|
| `server.host` | string | `"0.0.0.0"` | HTTP listen address |
| `server.port` | u16 | `3001` | HTTP listen port |
| `solana.rpc_url` | string | Required | Solana JSON RPC endpoint |
| `solana.channel_program_id` | string | Required | On-chain channel program ID (Base58) |
| `solana.keypair_path` | string | `""` | Ed25519 keypair file path; empty for auto-generation |
| `channel.default_tree_depth` | u32 | `4` | Default Merkle tree depth |
| `channel.default_challenge_duration` | u64 | `5000` | Default dispute period (slots) |
| `channel.default_min_challenge_delay` | u64 | `1000` | Minimum dispute delay (slots) |
| `channel.default_settle_window` | u64 | `10000` | Default settlement window (slots) |
| `channel.auto_close_offset` | u64 | `500000` | Auto-close offset (slots), 0 = no auto-close |
| `channel.db_path` | string | Required | sled database storage path |
| `compliance` | section | Optional | Compliance configuration; if omitted, compliance is disabled |

---

## 9. Security Checklist

| Check Item | Description | Status |
|:-----------|:------------|:-------|
| Secure key storage | Ed25519 private keys use a secure storage scheme | Required |
| Preimage confidentiality | HTLC preimages are not revealed before beneficiary confirmation | Required |
| sled data directory permissions | Restrict access to database files | Recommended |
| Sequence number continuity | Ensure no signing of LeafUpdates with a sequence lower than the current one | Required |
| Reasonable challenge_duration | Allow sufficient time to respond to disputes | Recommended |
| Amount conservation validation | Verify total amounts match before splitting the tree | Required |
| RPC endpoint security | Use private RPC or HTTPS in production | Recommended |
| Reverse proxy TLS | Enable HTTPS via Nginx in production | Required |
