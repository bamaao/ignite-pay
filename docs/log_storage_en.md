# Ignite Pay Log Storage Design

## Current State

| Component | Current Approach | Problem |
|------|---------|------|
| `ignite-pay-mcp` | `tracing` → stderr, controlled by `RUST_LOG` | Lost when process ends, no persistence |
| `ignite-pay-skill` | `tracing` → stderr | Same as above |
| `didcomm-router` | `tracing` → stderr | Same as above |
| `ignite_pay_app` (Dart) | `debugPrint` | Stripped by compiler in release builds, zero logging |
| `ignite-pay-state-channel` | Only `tracing::warn!` for HTLC persistence failures | No structured audit logging |

**Conclusion: The system currently has no log persistence capability.**

---

## Design Goals

1. **Financial-grade audit** — Full payment traceability to support dispute arbitration
2. **Privacy protection** — User transaction details are end-to-end encrypted; server/cloud cannot read them
3. **Tamper resistance** — Hash chains + Merkle roots ensure any deletion/modification is detectable
4. **Cross-device recovery** — Users can pull encrypted logs from the cloud after switching devices to rebuild local records
5. **Low cost** — Hot data stored locally, cold data compressed and uploaded to cloud, Merkle roots anchored on-chain

---

## Storage Architecture: Three-Tier Separation

```
┌──────────────────────────────────────────────────────┐
│                   L3 Cold Archive (On-Chain)         │
│         Merkle Root → Solana Program Log              │
│         or Celestia DA Layer                          │
│         Retention: Permanent                          │
├──────────────────────────────────────────────────────┤
│                   L2 Warm Storage (Cloud)             │
│         E2EE Encrypted LogChunk → IPFS (content-addressed) │
│         Index: ChunkManifest CID (user only needs to remember one CID) │
│         Retention: Permanent (IPFS pinning)           │
├──────────────────────────────────────────────────────┤
│                   L1 Hot Storage (Local)              │
│         Phone: SQLite                                 │
│         MCP/Skill: sled                               │
│         Plaintext / Lightweight Encryption            │
│         Retention: 7~15 days                          │
└──────────────────────────────────────────────────────┘
```

---

## Data Structure Definitions

### LogChunk (Protobuf)

```protobuf
syntax = "proto3";

package ignite_pay.audit.v1;

// Minimum unit of storage and synchronization
message LogChunk {
    // Metadata section (plaintext, for cloud indexing)
    ChunkMetadata metadata = 1;

    // Data section (EncryptedPayload encrypted with AES-256-GCM)
    bytes encrypted_payload = 2;

    // GCM authentication tag
    bytes auth_tag = 3;

    // Merkle root of all transactions in this chunk
    bytes merkle_root = 4;
}

message ChunkMetadata {
    string user_did = 1;           // User DID
    string provider_did = 2;       // Provider DID (MCP)
    uint64 chunk_id = 3;           // Monotonically increasing sequence number
    uint64 start_nonce = 4;        // Start nonce
    uint64 end_nonce = 5;          // End nonce
    bytes  prev_chunk_hash = 6;    // SHA-256 of previous chunk, forming a hash chain
    int64  timestamp_start = 7;    // Chunk start timestamp
    int64  timestamp_end = 8;      // Chunk end timestamp
}

// Ciphertext internal structure
message EncryptedPayload {
    repeated TransactionEntry entries = 1;
}

message TransactionEntry {
    uint64 nonce = 1;               // Globally monotonically increasing
    int64  delta_amount = 2;        // Change amount (lamports, positive=expense, negative=refund)
    uint64 cumulative_amount = 3;   // Cumulative total spending
    bytes  signature = 4;           // Provider signature (arbitration evidence)
    int64  timestamp = 5;           // Transaction timestamp
    string service_id = 6;          // Service identifier (e.g., API path)
    string payment_id = 7;          // Payment ID (links to auth-request)
    string merchant_did = 8;        // Merchant DID
    bytes  memo = 9;                // Optional memo
}
```

### ChunkManifest (Protobuf) — IPFS Index

```protobuf
// Manifest entry mapping a chunk to its IPFS CID
message ChunkManifestEntry {
    uint64 chunk_id = 1;
    string cid = 2;               // IPFS CID of the LogChunk
    bytes chunk_hash = 3;          // SHA-256 of serialized LogChunk
    bytes merkle_root = 4;         // Allows verification without downloading
}

// Manifest tracking all chunks for a user on IPFS
message ChunkManifest {
    string user_did = 1;
    repeated ChunkManifestEntry entries = 2;
    bytes prev_manifest_hash = 3;  // Hash chain for manifest integrity
}
```

Users only need to remember one manifest CID to locate all chunks.

### IPFS Sync Flow

```
Phone (LocalLogStore)
       │
       ▼
  Get unsynced entries
       │
       ▼
  build_chunk() → LogChunk (encrypted)
       │
       ▼
  upload_chunk(ipfs, chunk) → CID
       │
       ▼
  add_manifest_entry(manifest, chunk_id, cid, chunk)
       │
       ▼
  upload_manifest(ipfs, manifest) → manifest_cid
       │
       ▼
  mark_synced(end_nonce)
```

### IPFS Recovery Flow

```
New Device                              IPFS
  │                                        │
  │  1. download_manifest(manifest_cid)    │
  │ ──────────────────────────────────────>│
  │ <──────────────────────────────────────│  ChunkManifest
  │                                        │
  │  2. Sort entries by chunk_id           │
  │                                        │
  │  3. download_chunk(cid) one by one     │
  │ ──────────────────────────────────────>│
  │ <──────────────────────────────────────│  LogChunk (encrypted)
  │                                        │
  │  4. Verify chunk_hash matches manifest │
  │  5. Verify hash chain prev_chunk_hash  │
  │  6. decrypt_chunk() → TransactionEntry │
  │  7. Write to local SQLite              │
  │  8. Repeat 3-7 until complete          │
```

```
TransactionEntry[]
       │
       ▼
  Build Merkle Tree ──→ merkle_root
       │
       ▼
  Serialize EncryptedPayload (Protobuf)
       │
       ▼
  Zstd compression (estimated 5~10x compression ratio)
       │
       ▼
  AES-256-GCM encryption (key derived from user DID private key)
       │
       ▼
  Assemble LogChunk { metadata, encrypted_payload, auth_tag, merkle_root }
```

---

## Implementation Plan by Component

### 1. Phone (Flutter + Rust)

**L1 Local Storage: SQLite**

```rust
// ignite_pay_app/rust/src/api/log_store.rs

/// Local log storage (SQLite, L1 hot tier)
pub struct LocalLogStore {
    db: rusqlite::Connection,
    next_nonce: u64,
}

impl LocalLogStore {
    pub fn open(path: &str) -> Result<Self> { ... }

    /// Record a transaction
    pub fn record_transaction(&self, entry: &TransactionEntry) -> Result<()> { ... }

    /// Query the most recent N transactions
    pub fn recent_transactions(&self, limit: usize) -> Result<Vec<TransactionEntry>> { ... }

    /// Get current cumulative spending
    pub fn cumulative_spending(&self) -> Result<u64> { ... }

    /// Export entries not yet uploaded to cloud, for building LogChunk
    pub fn unsynced_entries(&self, since_chunk_id: u64) -> Result<Vec<TransactionEntry>> { ... }

    /// Mark entries as synced to L2
    pub fn mark_synced(&self, up_to_nonce: u64) -> Result<()> { ... }
}
```

**Log Trigger Points (insert into existing code):**

| Trigger Location | File | Recorded Content |
|---------|------|---------|
| Auth request received | `didcomm_service.dart` → `_decryptAndProcess` | `payment_id`, `merchant_did`, `amount` |
| Auth response sent | `didcomm_service.dart` → `sendAuthResponse` | `authorized`, `list_action` |
| Session key created | `didcomm_service.dart` → `sendAuthResponseWithSessionKey` | `spending_limit`, `duration` |
| Pairing completed | `didcomm_service.dart` → `parseInvitationAndConnect` | `mcp_did`, `mediator_ws_url` |
| WS/FCM connect/disconnect | `didcomm_service.dart` → `connectToMediator` / `disconnect` | Connection state change |

**L2 Upload (background task):**

```dart
// Triggered every 100 transactions or every 1 hour
Future<void> _syncLogChunk() async {
    final entries = await rust.getUnsyncedLogEntries(limit: 100);
    if (entries.isEmpty) return;

    // Done on Rust side: build Merkle → compress → encrypt → assemble LogChunk
    final chunk = await rust.buildLogChunk(entries: entries);

    // Upload to cloud (ciphertext only)
    await _cloudStorage.upload(chunk.metadata, chunk.encryptedPayload);

    // Mark local entries as synced
    await rust.markLogSynced(upToNonce: chunk.endNonce);
}
```

### 2. MCP Server (Rust)

**L1 Local Storage: sled (existing infrastructure)**

```rust
// ignite-pay-mcp/src/audit.rs

/// Server-side audit log (sled, L1 hot tier)
pub struct AuditLogStore {
    db: sled::Db,
}

impl AuditLogStore {
    /// Record payment event (no user-sensitive info, only channel-level State Diff)
    pub fn record_state_diff(
        &self,
        channel_id: &str,
        batch_id: &str,
        delta: u64,
        merkle_root: &[u8; 32],
    ) -> Result<()> { ... }

    /// Record authorization request/response event
    pub fn record_auth_event(
        &self,
        payment_id: &str,
        event_type: &str,  // "auth_request_sent" | "auth_response_received"
        metadata: &Value,
    ) -> Result<()> { ... }

    /// Query payment history
    pub fn query_payments(&self, from: i64, to: i64, limit: usize) -> Result<Vec<AuditEntry>> { ... }
}
```

**Log Trigger Points:**

| Trigger Location | File | Recorded Content |
|---------|------|---------|
| 402 challenge received | `main.rs` → `process_x402_challenge` | `merchant_did`, `amount`, `challenge_body` hash |
| Auth request sent | `mediator.rs` → `send_auth_request` | `payment_id`, `phone_did`, `merchant_did` |
| Auth response received | `mediator.rs` → `process_inner_message` | `authorized`, `list_action`, `session_key` presence |
| On-chain payment executed | `main.rs` → `execute_solana_payment` | `tx_signature`, `slot`, `amount` |
| Allowlist change | `main.rs` → `handle_list_action` | `list_type`, `action`, `merchant_did` |
| Pairing request | `mediator.rs` → `process_inner_message` | `phone_did`, `push_channel` |

**Log Format (Structured JSON):**

```json
{
  "ts": "2025-01-15T10:30:00Z",
  "level": "info",
  "event": "auth_request_sent",
  "payment_id": "pay_abc123",
  "merchant_did": "did:ignite:zMerchant...",
  "phone_did": "did:ignite:zPhone...",
  "amount": 500000000,
  "correlation_id": "req_xyz"
}
```

### 3. DIDComm Router (Mediator)

The Router does not log message content (it only relays encrypted messages); it logs operational logs:

```rust
// Structured tracing log, output to stderr + optional file
tracing::info!(
    recipient = %did,
    msg_count = messages.len(),
    "messages_queued"
);
```

Recommended addition of file output:

```rust
// didcomm-router/src/main.rs
let log_file = std::fs::File::create("logs/router.log")?;
tracing_subscriber::fmt()
    .with_writer(std::io::stderr.and(log_file))
    .with_rolling_file_appender(Rotation::DAILY, "logs", "router.log")
    .init();
```

---

## Hash Chain and Merkle Reconciliation

### Hash Chain (Tamper Detection)

Each `LogChunk`'s `metadata.prev_chunk_hash` points to the SHA-256 of the previous chunk.

```
Chunk#0 ← prev_hash=0x00..00
Chunk#1 ← prev_hash=SHA256(Chunk#0)
Chunk#2 ← prev_hash=SHA256(Chunk#1)
...
```

Verification during sync: if `SHA256(Chunk#N) != Chunk#N+1.prev_chunk_hash`, data tampering or deletion is detected.

### Merkle Reconciliation (Server vs User)

```
User side:    Transaction[] → Merkle Tree → merkle_root_user
Server side:  StateDiff[]   → Merkle Tree → merkle_root_server

Verify: merkle_root_user == merkle_root_server
```

Existing code foundation: `ignite-pay-state-channel/src/merkle.rs` already implements a complete Merkle tree (construction, incremental updates, proof generation/verification), which can be directly reused.

### On-Chain Anchoring (L3)

Write the `merkle_root` as an account data field in a Solana custom instruction, or publish to the Celestia DA layer.

Existing code foundation: `ignite-pay-solana/src/compression.rs` already has SPL Concurrent Merkle Tree interaction capability.

---

## Key Derivation Scheme

The client-side encryption key is derived from the DID private key, requiring no additional key management:

```
Ed25519 Signing Private Key (32 bytes)
    │
    ▼  HKDF-SHA256(salt="ignite-pay-log-v1", info=user_did)
    │
    ▼
AES-256-GCM Key (32 bytes)
```

```rust
// ignite-pay-core/src/log_crypto.rs (new file)

use hkdf::Hkdf;
use sha2::Sha256;

/// Derive AES-256-GCM encryption key from Ed25519 signing private key
pub fn derive_log_key(signing_private: &[u8; 32], user_did: &str) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(
        Some(b"ignite-pay-log-v1"),
        signing_private,
    );
    let mut key = [0u8; 32];
    hk.expand(user_did.as_bytes(), &mut key).expect("32 bytes");
    key
}
```

---

## Cross-Device Recovery Flow

```
New Device                    IPFS                         Old Device
  │                            │                             │
  │  1. User enters DID + private key                        │
  │    + manifest CID          │                             │
  │                            │                             │
  │  2. download_manifest(cid) │                             │
  │ ──────────────────────────>│                             │
  │ <──────────────────────────│                             │
  │  (ChunkManifest)           │                             │
  │                            │                             │
  │  3. Sort by chunk_id       │                             │
  │                            │                             │
  │  4. download_chunk(cid)    │                             │
  │ ──────────────────────────>│                             │
  │  5. Return encrypted chunk │                             │
  │ <──────────────────────────│                             │
  │                            │                             │
  │  6. Verify chunk_hash      │                             │
  │  7. Derive key → decrypt   │                             │
  │  8. Verify hash chain integrity                         │
  │  9. Verify Merkle root     │                             │
  │ 10. Write to local SQLite  │                             │
  │ 11. Repeat 4-10 until done │                             │
```

---

## New Dependencies Required

### ignite-pay-core / Cargo.toml

```toml
# Log storage
prost = "0.13"              # Protobuf runtime
prost-types = "0.13"        # protobuf well-known types
hkdf = "0.12"               # Key derivation
aes-gcm = "0.10"            # AES-256-GCM encryption
zstd = "0.13"               # Compression
sha2 = "0.10"               # SHA-256 (already present, confirm version)
```

### ignite_pay_app / rust / Cargo.toml

```toml
rusqlite = { version = "0.31", features = ["bundled"] }  # SQLite
```

### ignite-pay-mcp / Cargo.toml

```toml
tracing-appender = "0.2"    # File log rotation
```

---

## Implementation Roadmap

### Phase 1: Basic Log Persistence

1. **MCP Audit Log** — Add `audit.rs`, sled storage, insert structured writes alongside existing `tracing::info!` trigger points
2. **Phone Local Log** — Add `log_store.rs` (SQLite), insert Rust bridge calls at key operation points in the Dart layer
3. **Router File Log** — Add `tracing-appender` file output

### Phase 2: E2EE Log Stream

4. **Protobuf Definitions** — Add `proto/audit.proto`, `build.rs` generates Rust code
5. **Key Derivation** — Add `log_crypto.rs`, HKDF derives AES key from DID private key
6. **Chunk Construction** — Add `log_chunk.rs`, implement Merkle construction → Zstd compression → AES encryption → hash chain

### Phase 3: IPFS Cloud Sync

7. **IPFS Upload** — Upload encrypted LogChunk to IPFS to obtain CID, use `ChunkManifest` to record mappings
8. **Cross-Device Recovery** — From manifest CID → pull manifest → pull chunks one by one → decrypt → verify hash chain
9. **On-Chain Anchoring** — MCP periodically writes Merkle roots to Solana program logs or Celestia

---

## Relationship to Existing Code

| Design Element | Existing Code Foundation | New Additions Needed |
|---------|------------|--------|
| Local storage | `sled` (MCP), `SharedPreferences` (Phone) | `rusqlite` (Phone SQLite) |
| Merkle tree | `state-channel/src/merkle.rs` complete implementation | Reuse, wrap as `audit_merkle` |
| Encryption | DIDComm authcrypt (`affinidi_messaging_didcomm`) | AES-256-GCM (independent of DIDComm) |
| Hash chain | None | `prev_chunk_hash` logic |
| Compression | None | `zstd` crate |
| IPFS | `ipfs.rs` trait + `KuboIpfsClient` | Optional: L2 via IPFS instead of S3 |
| On-chain writes | `compression.rs` SPL Merkle Tree | Merkle root anchoring logic |
| Compliance audit | `compliance.rs` audit trail | Extend to global log format |
| Allowlist/Blocklist | `list_store.rs` sled implementation | Log allowlist change events |
| Protobuf | None | `prost` + `.proto` files |

---

## Security Considerations

1. **Key isolation** — Log encryption keys are separated from DIDComm communication keys (differentiated via HKDF salt)
2. **Cloud zero-knowledge** — Cloud only stores `{metadata, encrypted_payload}`, with no ability to decrypt
3. **Forward secrecy** — Each chunk uses an independent IV/nonce, so even if a single key is compromised, other chunks remain unaffected
4. **Release build logging** — Replace Dart-side `debugPrint` with persistent logging (`debugPrint` is stripped in release builds)
5. **Automatic log cleanup** — When L1 reaches the 500MB limit, replace synced data with Merkle root digests to free space
