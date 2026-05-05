# State Channel Merchant Deployment Configuration Guide

## 1. Overview

The merchant side (Party B, payee) is the service provider of the state channel, responsible for receiving user payments, co-signing confirmations, managing HTLC preimages, and claiming leaves during settlement. The merchant uses the `channel-provider` binary deployed as a continuously running server process.

Two deployment modes are supported:
- **Mode 1 (Recommended)**: Run as a standalone HTTP service using the `channel-provider` binary from `ignite-pay-channel-service`
- **Mode 2**: Integrate into your own service via the `ignite-pay-state-channel` library

---

## 2. Service Deployment

### 2.1 Build

```bash
cd ignite-pay-channel-service
cargo build --release --bin channel-provider
```

Artifact: `target/release/channel-provider`

### 2.2 Generate Key

```bash
solana-keygen new --outfile ./keys/provider.key
```

> If `keypair_path` is left empty, the service will automatically generate a temporary key at startup (for testing only).

### 2.3 Configuration File

Create `config-provider.toml`:

```toml
[server]
host = "0.0.0.0"        # Listen address; for production, use "127.0.0.1" + reverse proxy
port = 3002              # Listen port

[solana]
rpc_url = "https://api.devnet.solana.com"
channel_program_id = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe"
keypair_path = "./keys/provider.key"

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 0
db_path = "./data/channel_provider"
```

> The Provider role does not require a `[compliance]` configuration section; compliance is managed by the User side.

### 2.4 Start the Service

```bash
# Use the default configuration file config-provider.toml
./channel-provider

# Specify a configuration file
./channel-provider /path/to/config-provider.toml

# Enable debug logging
RUST_LOG=debug ./channel-provider
```

### 2.5 API Endpoints

#### General Endpoints

| Method | Path | Description |
|:-------|:-----|:------------|
| GET | `/health` | Health check |
| WS | `/ws` | WebSocket connection |

#### Channel Management Endpoints

| Method | Path | Description |
|:-------|:-----|:------------|
| POST | `/v1/channels/{id}/fund` | Fund the channel (merchant deposit) |
| GET | `/v1/channels` | List channels |
| GET | `/v1/channels/{id}` | Query channel status |

#### Payment Processing Endpoints

| Method | Path | Description |
|:-------|:-----|:------------|
| POST | `/v1/channels/{id}/cosign` | Provider co-sign |
| POST | `/v1/channels/{id}/accept-payment` | Accept payment |
| POST | `/v1/channels/{id}/accept-batch` | Accept batch payment |

#### Settlement Endpoints

| Method | Path | Description |
|:-------|:-----|:------------|
| POST | `/v1/channels/{id}/close` | Cooperative close |
| POST | `/v1/channels/{id}/challenge` | Initiate dispute |
| POST | `/v1/channels/{id}/submit-counter` | Submit counter-state |
| POST | `/v1/channels/{id}/claim` | Claim leaf |
| POST | `/v1/channels/{id}/finalize` | Final settlement |

### 2.6 Example Requests

```bash
# Health check
curl http://localhost:3002/health

# Merchant funds the channel
curl -X POST http://localhost:3002/v1/channels/{channel_id}/fund \
  -H "Content-Type: application/json" \
  -d '{
    "amount": 500000,
    "token_mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
  }'

# Accept payment (verify and apply the user's LeafUpdate)
curl -X POST http://localhost:3002/v1/channels/{channel_id}/accept-payment \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_update": {
      "channel_id": "hex...",
      "sequence": 5,
      "leaf_index": 2,
      "prev_leaf_hash": "hex...",
      "new_leaf": { ... },
      "signature": [64 bytes]
    }
  }'

# Provider co-sign
curl -X POST http://localhost:3002/v1/channels/{channel_id}/cosign \
  -H "Content-Type: application/json" \
  -d '{
    "sequence": 5,
    "root": "hex..."
  }'

# Accept batch payment
curl -X POST http://localhost:3002/v1/channels/{channel_id}/accept-batch \
  -H "Content-Type: application/json" \
  -d '{
    "updates": [
      { "channel_id": "hex...", "sequence": 5, ... },
      { "channel_id": "hex...", "sequence": 6, ... }
    ]
  }'

# Cooperatively close the channel
curl -X POST http://localhost:3002/v1/channels/{channel_id}/close \
  -H "Content-Type: application/json" \
  -d '{
    "sequence": 10,
    "root": "hex...",
    "signature_a": [64 bytes],
    "signature_b": [64 bytes]
  }'

# Claim leaf
curl -X POST http://localhost:3002/v1/channels/{channel_id}/claim \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 1,
    "leaf_amount": 500000,
    "leaf_hash": "hex...",
    "leaf_data": "hex...",
    "leaf_owner": "Merchant Solana public key",
    "proof": ["hex...", "hex...", "hex...", "hex..."],
    "claimer_signature": [64 bytes]
  }'

# Submit counter-state (dispute response)
curl -X POST http://localhost:3002/v1/channels/{channel_id}/submit-counter \
  -H "Content-Type: application/json" \
  -d '{
    "sequence": 10,
    "root": "hex...",
    "signature_a": [64 bytes],
    "signature_b": [64 bytes]
  }'
```

### 2.7 WebSocket Real-Time Communication

The merchant side supports WebSocket connections for real-time reception of user LeafUpdates, co-sign requests, and HTLC status changes.

```javascript
const ws = new WebSocket('ws://localhost:3002/ws');

// Authentication
ws.onopen = () => {
  const timestamp = Date.now();
  const message = `channel-ws-auth:${timestamp}`;
  const signature = await ed25519.sign(sha256(message), privateKey);

  ws.send(JSON.stringify({
    type: 'auth',
    pubkey: base58Encode(publicKey),
    signature: Array.from(signature),
    timestamp
  }));
};

ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);
  switch (msg.type) {
    case 'leaf_update':
      // Handle the user's LeafUpdate
      break;
    case 'cosign_request':
      // Respond to co-sign request
      break;
    case 'htlc_preimage':
      // Handle HTLC preimage reveal
      break;
  }
};
```

For the detailed WebSocket protocol, see [Scenario 12: WebSocket Real-Time Communication](scenarios/12-websocket.md).

### 2.8 systemd Service

```ini
[Unit]
Description=Ignite Pay Channel Provider Service
After=network.target

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/channel-provider /opt/ignite-pay/config-provider.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

### 2.9 Nginx Reverse Proxy

```nginx
server {
    listen 443 ssl;
    server_name merchant.ignite-pay.example.com;

    ssl_certificate     /etc/ssl/certs/ignite-pay.pem;
    ssl_certificate_key /etc/ssl/private/ignite-pay.key;

    location / {
        proxy_pass http://127.0.0.1:3002;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }

    location /ws {
        proxy_pass http://127.0.0.1:3002;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
    }
}
```

---

## 3. Configuration Parameter Details

| Parameter | Type | Description |
|:----------|:-----|:------------|
| `server.host` | string | HTTP listen address |
| `server.port` | u16 | HTTP listen port (default 3002) |
| `solana.rpc_url` | string | Solana JSON RPC endpoint |
| `solana.channel_program_id` | string | On-chain channel program ID |
| `solana.keypair_path` | string | Ed25519 keypair file path |
| `channel.db_path` | string | sled database path |
| `channel.default_tree_depth` | u32 | Default Merkle tree depth |
| `channel.auto_close_offset` | u64 | Auto-close offset (0 = no auto-close) |

---

## 4. Monitoring Recommendations

| Metric | Threshold | Action |
|:-------|:----------|:-------|
| Active channel count | Trend change | Monitor business volume changes |
| Co-sign latency | > 500ms | Optimize network or node performance |
| Claim rate within settlement window | < 100% | Check if claim logic is timely |
| HTLC expiry rate | > 1% | Check preimage reveal process |
| sled database size | > 2 GB | Archive historical data |
| Payment acceptance failure rate | > 0.1% | Check signature verification logic |

---

## 5. DID Digital Identity

### 5.1 Generate DID Keypair

The merchant uses the `identity` module from `ignite-pay-core` to generate a `did:ignite` decentralized identity:

```toml
[dependencies]
ignite-pay-core = { path = "../ignite-pay-core" }
ignite-pay-state-channel = { path = "../ignite-pay-state-channel" }
solana-pubkey = "2"
solana-program = "2"
ed25519-dalek = "1"
```

```rust
use ignite_pay_core::identity::{generate_ignite_did, build_did_document, save_identity, load_did};

let db = sled::open("./merchant_data")?;

// Check if identity already exists
let existing_did = load_did(&db)?;

let (identity, merchant_did) = match existing_did {
    Some(did) => {
        // DID already exists, regenerate identity with the same DID
        // Note: keys will be different, but the DID identifier remains the same
        let identity = PrivateIdentity::generate(&did);
        (identity, did)
    }
    None => {
        // Generate for the first time
        let (identity, did) = generate_ignite_did();
        save_identity(&db, &identity, &did)?;
        (identity, did)
    }
};

// Build W3C DID Document
let did_doc = build_did_document(&merchant_did, &identity);

println!("Merchant DID: {}", merchant_did);
// Output similar to: did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK
```

**DID Encoding Rule**:

`did:ignite:z` + Base58(`0xed 0x01` + Ed25519 public key)

where `0xed 0x01` is the multicodec identifier prefix for Ed25519 public keys.

**After generation, you obtain**:
- **DID Identifier**: `did:ignite:z6Mk...`
- **Ed25519 Signing Private Key**: Used to sign payment requests and DIDComm messages (store securely)
- **X25519 Key Agreement Key**: Used for DIDComm encrypted communication (derived from Ed25519)
- **Solana Payment Keypair**: An independent Solana keypair used to receive payments

> **Important**: The DID signing key and the Solana payment key are separate. The DID key is used for identity authentication, while the Solana payment key is used to receive funds.

### 5.2 DID Document Structure

```json
{
  "@context": ["https://www.w3.org/ns/did/v1"],
  "id": "did:ignite:z6MkhaXgBZ...",
  "verificationMethod": [{
    "id": "did:ignite:z6MkhaXgBZ...#key-signing-1",
    "type": "Ed25519VerificationKey2020",
    "controller": "did:ignite:z6MkhaXgBZ...",
    "publicKeyMultibase": "z6MkhaXgBZ..."
  }],
  "keyAgreement": [{
    "id": "did:ignite:z6MkhaXgBZ...#key-agreement-1",
    "type": "X25519KeyAgreementKey2020",
    "controller": "did:ignite:z6MkhaXgBZ...",
    "publicKeyBase64": "base64-encoded-x25519-public-key"
  }]
}
```

### 5.3 Apply for Platform Endorsement (VC)

Submit the following information to the Ignite Pay platform:

| Field | Description |
|:------|:------------|
| `merchant_did` | Merchant DID identifier |
| `name` | Merchant name |
| `category` | Merchant category (e.g., SaaS, API, Content) |
| `service_endpoint` | Merchant service URL |
| `solana_pubkey` | Solana payment public key |

After platform review, the `vc` module from `ignite-pay-core` is used to issue a Verifiable Credential:

```rust
use ignite_pay_core::vc::VerifiableCredential;
use ed25519_dalek::SigningKey;

// Platform-side VC issuance
let vc = VerifiableCredential::sign(
    vec!["https://www.w3.org/2018/credentials/v1".to_string()],
    "vc:ignite:merchant:001".to_string(),
    vec!["VerifiableCredential".to_string(), "MerchantAttestation".to_string()],
    platform_did.clone(),                          // Issuer: Platform DID
    chrono::Utc::now() - chrono::Duration::hours(1),
    chrono::Utc::now() + chrono::Duration::days(365),
    merchant_did.clone(),                          // Subject: Merchant DID
    "Example Merchant".to_string(),
    Some("SaaS".to_string()),
    &platform_signing_key,                         // Platform signing private key
    &format!("{}#key-signing-1", platform_did),
);
```

Generated VC format:

```json
{
  "@context": ["https://www.w3.org/2018/credentials/v1"],
  "type": ["VerifiableCredential", "MerchantAttestation"],
  "issuer": "did:ignite:zPlatformDID...",
  "issuanceDate": "2025-01-01T00:00:00Z",
  "expirationDate": "2026-01-01T00:00:00Z",
  "credentialSubject": {
    "id": "did:ignite:z6MkMerchant...",
    "name": "Example Merchant",
    "category": "SaaS"
  },
  "proof": {
    "type": "Ed25519Signature2020",
    "verificationMethod": "did:ignite:zPlatformDID...#key-signing-1",
    "proofValue": "base64-signature..."
  }
}
```

### 5.4 On-Chain Registration (SPL Account Compression)

The platform compresses merchant information on-chain using `SolanaDidBridge` from `ignite-pay-core` (requires the `solana` feature to be enabled):

```rust
use ignite_pay_core::solana_did::SolanaDidBridge;

// Platform-side: Register merchant to the on-chain Merkle Tree
// MerchantLeaf {
//     merchant_did_hash: SHA-256(Merchant DID),
//     active_pubkey: Solana payment address,
//     platform_vc_hash: SHA-256(canonical_json(VC)),
//     status: 0,  // 0=active
//     slot_updated: current_slot
// }
```

On-chain parameters (deployed once by the platform):

| Parameter | Value | Description |
|:----------|:------|:------------|
| Merkle Tree Address | Solana Pubkey | ConcurrentMerkleTree account |
| Tree Authority | Solana Pubkey | Platform control key |
| maxDepth | 14 | Supports ~16K merchants |
| maxBufferSize | 64 | Concurrent update buffer |
| DAS API | Helius endpoint | Used for querying Merkle Proofs |

### 5.5 DID Persistence

```rust
use ignite_pay_core::identity::{save_identity, load_did};

// Save to sled
save_identity(&db, &identity, &merchant_did)?;

// Load after restart
let did = load_did(&db)?;
```

> **Note**: The seed of the current `PrivateIdentity` cannot be directly extracted, so `load_did` only restores the DID string. Keys are regenerated on restart (the DID remains unchanged). Production environments require additional implementation of secure key persistence.

---

## 6. Roles and Responsibilities

| Responsibility | Description |
|:---------------|:------------|
| DID Identity Management | Generate/persist did:ignite identity, maintain DID Document |
| Receive Payments | Receive user-signed LeafUpdates, co-sign to confirm |
| HTLC Management | Generate preimages, reveal preimages after service delivery |
| Provider Co-signing | Ed25519 sign user LeafUpdates and SignedStates |
| Settlement Claims | Submit Merkle Proofs to claim own leaves within the settlement window |
| Dispute Response | Submit counter_state within the window after receiving a challenge |
| VC Renewal | Periodically check VC validity and apply for renewal before expiration |

---

## 7. Channel Integration (Library Integration Mode)

### 7.1 Initialize ChannelManager

```rust
use ignite_pay_state_channel::channel::ChannelManager;
use ignite_pay_state_channel::signing::{generate_keypair, to_pubkey};

let db = sled::open("./merchant_channel_data")?;
let manager = ChannelManager::new(db)?;

// Merchant channel keypair (independent of DID key)
let provider_keypair = generate_keypair();
let provider_pubkey = to_pubkey(&provider_keypair);
```

### 7.2 Load Channel

After a user opens a channel, the merchant obtains the `channel_id` from on-chain events or the communication protocol and loads the channel state:

```rust
let channel_id: [u8; 32] = /* obtained from on-chain or communication */;
let state = manager.load_state(&channel_id)?;

println!("Channel status: {:?}", state.metadata.status);
println!("User deposit: {}", state.metadata.deposit_a);
```

### 7.3 Provider Funding (Optional)

If the channel requires dual-party funding, the merchant can inject funds:

```rust
let update = manager.fund_channel(
    &mut state,
    &provider_keypair,
    500_000,        // Merchant funding amount
    None,           // Automatically select an empty slot
)?;

// update is a signed LeafUpdate that needs to be submitted to the on-chain fund_channel instruction
```

---

## 8. Processing User Payments (Library Integration Mode)

### 8.1 Receive LeafUpdate

The merchant receives the LeafUpdate sent by the user, verifies the signature, and applies it:

```rust
use ignite_pay_state_channel::signing::verify_leaf_update_signature;

// Verify signature
if !verify_leaf_update_signature(&leaf_update, &state.metadata.user_pubkey) {
    return Err("Invalid user signature");
}

// Apply update
manager.apply_leaf_update(&mut state, &leaf_update, &state.metadata.user_pubkey)?;
```

### 8.2 Provider Co-sign

The merchant co-signs the updated state, indicating agreement with the new state:

```rust
let cosignature = manager.provider_cosign_state(
    &mut state,
    &provider_keypair,
)?;

// cosignature is the merchant's Ed25519 signature
// Return to the user as confirmation
```

### 8.3 Batch Update Processing

```rust
// The user may send multiple LeafUpdates at once
let updates: Vec<LeafUpdate> = /* obtained from communication */;

let result = manager.apply_leaf_update_batch(
    &mut state,
    &updates,
    &state.metadata.user_pubkey,
)?;

// If an intermediate update in the batch fails, result is Err(BatchFailureInfo)
// Already applied updates are not automatically rolled back (requires cooperative handling)
```

---

## 9. HTLC Management (Library Integration Mode)

### 9.1 Reveal Preimage After Service Completion

After the merchant has delivered the service, they need to reveal the HTLC preimage to complete the payment:

```rust
use ignite_pay_state_channel::htlc::HtlcManager;

let mut htlc_mgr = HtlcManager::with_db(db.clone(), channel_id);

// The merchant holds the preimage (obtains hash_lock from the user, user reveals preimage after service completion)
// Or: the merchant generates the preimage

// Method A: User creates HTLC, merchant waits for preimage
// User sends hash_lock -> Merchant verifies -> User reveals preimage after service completion

// Method B: Merchant generates preimage
let (hash_lock, preimage) = htlc_mgr.create_htlc(
    100_000,           // Amount
    leaf_index,        // Leaf index
    user_pubkey,       // Owner (user locks funds)
    provider_pubkey,   // Beneficiary (merchant)
    current_slot,
    500,               // Duration
);

// Send hash_lock to the user (user creates HTLC leaf)
// Reveal preimage after service completion
htlc_mgr.reveal_preimage(&hash_lock, &preimage)?;
```

### 9.2 Check Expiry

```rust
// Periodically check for expired HTLCs
let expired = htlc_mgr.check_expiry(current_slot);
for hash_lock in &expired {
    htlc_mgr.mark_refunded(hash_lock)?;
}
```

### 9.3 HTLC Lifecycle

```
Pending -> (preimage reveal) -> Revealed -> (on-chain resolution) -> Fulfilled
Pending -> (timeout) -> Expired -> (refund) -> Refunded
```

---

## 10. Settlement Operations (Library Integration Mode)

### 10.1 Claim Leaf

Within the settlement window, the merchant submits a Merkle Proof to claim their UTXO:

```rust
use ignite_pay_state_channel::signing::claim_message;

// Get the leaf owned by the merchant
let leaf_index = 1;  // Leaf index owned by the merchant
let leaf = state.tree.get_leaf(leaf_index)?;

// Generate Merkle Proof
let proof = state.tree.proof(leaf_index)?;

// Construct on-chain claim call parameters
let claim_amount = leaf.amount;
let leaf_hash = leaf.hash();
let leaf_data = borsh::to_vec(leaf)?;
let leaf_owner = leaf.owner;  // Should be provider_pubkey

// Sign (off-chain helper function; on-chain verification uses channel_id || current_slot || current_root)
let claim_msg = claim_message(&channel_id, leaf_index as u32, claim_amount, current_slot);
let signature = provider_keypair.sign(&claim_msg);
```

### 10.2 HTLC Claim

If the leaf is of HTLC type, use the on-chain `verify_htlc` instruction:

```rust
// Need to provide:
// - leaf_index
// - preimage (32 bytes)
// - hash_lock
// - leaf_amount
// - beneficiary (should be provider_pubkey)
// - leaf_hash + Merkle proof
// - timelock_slot (must be >= current_slot)
// - leaf_data
// - claimer_signature
```

> **Deadline**: In the `Challenged` state, the deadline is `challenge_slot + challenge_duration` (`settle_deadline` is None); in the `Settling` state, use `settle_deadline`.

### 10.3 HTLC Refund

If the HTLC has expired, the merchant does not claim it (funds are returned to the user), or the user uses the `htlc_refund` instruction:

```rust
// Requires: timelock_slot < current_slot
// User submits the htlc_refund instruction, funds are returned to leaf.owner
```

### 10.4 Final Settlement

After the settlement window ends, either party calls `finalize_settlement`:

```rust
// On-chain operations:
// - Calculate unclaimed balance
// - Distribute proportionally by deposit_a / deposit_b ratio
// - Transfer remaining funds to vault_a and vault_b respectively
// - Close the channel
```

---

## 11. Compliance Support (Library Integration Mode)

### 11.1 Spending Limits

If compliance management is enabled for the channel:

```rust
use ignite_pay_state_channel::compliance::{ComplianceManager, SpendingLimit};

let compliance = ComplianceManager::new(db.clone())?;

compliance.init_channel_compliance(channel_id, SpendingLimit {
    threshold: 1_000_000,     // Cumulative spending threshold
    per_channel: 2_000_000,   // Maximum payment per channel
    window_slots: 1000,       // Sliding window (slots)
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
    ComplianceAction::None => { /* Normal */ }
    ComplianceAction::InsertMarker { compliance_hash, threshold } => {
        // Trigger compliance review; need to insert a compliance marker leaf
    }
}
```

### 11.2 Audit Trail

```rust
// Record each LeafUpdate
compliance.record_audit(&leaf_update)?;

// Query full channel audit
let trail = compliance.get_audit_trail(channel_id)?;
for update in &trail {
    println!("seq={} leaf_idx={} amount={}",
        update.sequence, update.leaf_index, update.new_leaf.amount);
}
```

---

## 12. Key Rotation

When the merchant needs to change their Solana payment address:

1. Use the DID signing private key to sign a new payment address declaration
2. Submit to the platform
3. After the platform verifies the DID signature, call `replace_leaf` to update the on-chain leaf node
4. `merchant_did_hash` remains unchanged, DID identifier remains unchanged
5. The new payment address takes effect in the next slot

> The DID identifier and signing key remain unchanged; only the payment address is replaced, ensuring business continuity.

---

## 13. Multi-Channel Management

Merchants typically maintain channels with multiple users simultaneously:

```rust
// Use sled prefix to manage multiple channels
let db = sled::open("./merchant_channels")?;
let manager = ChannelManager::new(db)?;

// Load a specific channel
let channel_id = /* ... */;
let state = manager.load_state(&channel_id)?;

// Manage multiple HTLCs (each channel is independent)
let htlc_mgr = HtlcManager::with_db(db.clone(), channel_id_1);
let htlc_mgr_2 = HtlcManager::with_db(db.clone(), channel_id_2);
```

---

## 14. Security Checklist

| Check Item | Description | Status |
|:-----------|:------------|:-------|
| DID signing private key secure storage | Use HSM or key management service | Required |
| Solana payment private key secure storage | Periodically check balance and transfer to cold wallet | Recommended |
| VC validity check | Periodically renew platform VC | Operations note |
| Preimage management | Only reveal preimages after service confirmation | Required |
| LeafUpdate verification | Verify user signature before each co-sign | Required |
| Sequence number check | Only accept updates where sequence > current value | Required |
| Amount conservation | Verify total amount remains unchanged after each update | Required |
| Settlement window monitoring | Claim leaves within the window in a timely manner | Required |
| HTLC timeout handling | Periodically check for expired HTLCs and process refunds | Recommended |
| Audit completeness | Record all LeafUpdates | Recommended |
| Payment address monitoring | Monitor on-chain payment addresses to detect anomalous transactions promptly | Recommended |
