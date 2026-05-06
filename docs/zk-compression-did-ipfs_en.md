# Technical Architecture: ZK Compression, DID On-Chain Registration, and IPFS Storage

## 1. Overview

The Ignite-Pay system employs a three-layer decentralized storage architecture:

| Layer | Technology | Purpose |
|-------|-----------|---------|
| **On-chain Compression Layer** | ZK Compression (Light Protocol) | Compressed DID storage, on-chain verification |
| **Off-chain Identity Layer** | did:ignite method + VC | Decentralized identity and verifiable credentials |
| **Distributed Storage Layer** | IPFS (Kubo) | VC storage, policy list sync, audit log backup |

---

## 2. The Role of ZK Compression

### 2.1 Core Positioning

ZK Compression (based on Light Protocol) is **not an optional optimization layer** — it is the **sole mechanism** for on-chain merchant DID storage. Merchant DID data never exists as a traditional Solana account. Instead, it is stored as a compressed account hash in Light Protocol's Merkle trees.

### 2.2 Storage Model

```
ConcurrentMerkleTree (State Tree)
└── Leaf: Hash(MerchantCompressedDid)
    ├── original_pk    — Identity anchor (immutable)
    ├── controller_pk  — Controller key (rotatable)
    ├── recovery_pk    — Recovery key
    ├── vc_hash        — Platform VC SHA-256 hash
    ├── last_updated   — Last update timestamp
    └── nonce          — Anti-replay counter
```

The struct is defined in `ignite-pay-did-program/src/state.rs`, approximately 150 bytes, stored as a hash in a Merkle tree leaf node.

### 2.3 Key Technical Elements

**Deterministic Addressing**:
```
compressed_address = derive_address([b"merchant-did", original_pk], address_tree, program_id)
```
Each merchant has exactly one compressed address, derived from the original public key.

**Light System Program CPI**: All on-chain write operations (initialize, update VC, set recovery key, recover controller) write to the Merkle tree via Light System Program CPI.

**Validity Proof**: Every mutation operation requires a ZK validity proof from the Photon RPC (Light Protocol indexer service), embedded in the instruction data and verified by the Light System Program during CPI execution.

### 2.4 Advantages

- **No rent exemption required**: Compressed accounts do not need the traditional Solana rent deposit.
- **Massive scalability**: A single Merkle tree can store thousands of merchant DIDs.
- **Low transaction costs**: Far cheaper than creating individual on-chain accounts.
- **Privacy**: Only hashes are stored on-chain; full VC data remains off-chain.

---

## 3. DID On-Chain Registration Flow

### 3.1 The did:ignite DID Method

DIDs are generated locally and do not require on-chain registration:
- Format: `did:ignite:z<multibase-base58btc>`
- Encoded content: `0xed 0x01` (multicodec Ed25519 prefix) + 32-byte Ed25519 public key
- Example: `did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK`

### 3.2 Three-Layer Key Architecture

| Key | Purpose | Mutability |
|-----|---------|-----------|
| **Original Key** (`original_pk`) | Identity anchor, PDA derivation seed | Immutable |
| **Controller Key** (`controller_pk`) | Signs daily operations (update VC, set recovery) | Rotatable via `rotate-key` |
| **Recovery Key** (`recovery_pk`) | Disaster recovery — can reset controller | Set via `set_recovery_key` |

### 3.3 End-to-End Registration Flow

```
Step 1: Merchant generates identity locally
    Generate Ed25519 keypair → Derive did:ignite:z... identifier

Step 2: Request VC from platform
    GET /v1/auth/nonce → Get anti-replay nonce (5-minute TTL)
    Merchant signs "issue_vc:{did}:{merchant_name}:{nonce}"
    POST /v1/vc/issue → Platform verifies DID ownership, issues W3C VC

Step 3: Register on-chain as compressed DID
    Merchant signs "register:{did}:{pubkey}:{vc_hash}:{nonce}"
    POST /v1/merchants/register → Platform signs (credential_subject_pk || vc_hash)
    Platform creates compressed account via Light System Program CPI

Step 4 (SelfOnchain mode only)
    POST /v1/proof → Merchant obtains ZK proof
    Merchant builds, signs, and broadcasts transaction
    POST /v1/merchants/confirm → Merchant notifies platform
```

### 3.4 On-Chain Verification (Within the Solana Program)

The `initialize_did` instruction performs triple verification:

1. **Subject Binding**: `credential_subject_pk == signer.key()` — ensures the transaction submitter is the VC subject
2. **Platform Signature Verification**: `verify(platform_pubkey, credential_subject_pk || vc_hash, platform_signature)` — proves the platform authorized this binding
3. **Deterministic Address Derivation**: ensures each merchant has exactly one address

### 3.5 Dual On-Chain Modes

| Mode | Description |
|------|-------------|
| **Sponsored** (default) | Platform signs and sends the transaction, records a service fee |
| **SelfOnchain** | Platform builds an unsigned transaction, merchant signs and broadcasts |

### 3.6 Bridge Layer

`SolanaDidBridge` (`ignite-pay-core/src/solana_did.rs`) connects the core identity module to the Solana compression layer:

```
SolanaDidBridge.quick_verify():
    1. Extract Ed25519 public key from did:ignite:z... identifier
    2. Derive compressed PDA address via DidService::derive_compressed_address
    3. Query Photon API getCompressedAccount to confirm compressed account exists
```

---

## 4. The Role of IPFS

### 4.1 Three Core Purposes

IPFS serves three distinct but complementary functions in the system:

| Purpose | Data Type | Consumer |
|---------|-----------|----------|
| VC Storage & Resolution | Verifiable Credential JSON | MCP server (during payment verification) |
| Policy List Sync | Whitelist/Blacklist JSON | Phone App (cross-device sync) |
| Audit Log Backup | Encrypted log protobuf | Phone App (device migration/restore) |

### 4.2 VC Storage and Resolution

**Storage**: Platform-issued VCs are uploaded to IPFS, receiving a CID.

**Reference**: X402 payment requests reference VCs via the `vc_ipfs_cid` field:
```json
{
  "vc_ipfs_cid": "bafyreib4pdl7kg...vfqr3q"
}
```

**Resolution**: The MCP server calls `resolve_vc_from_ipfs()` during payment verification to download and verify the VC from IPFS.

**Code locations**:
- `ignite-pay-core/src/vc.rs` — `resolve_vc_from_ipfs()`
- `ignite-pay-mcp/src/main.rs` — IPFS CID path in payment flow (~line 706-764)
- `ignite-pay-mcp/src/tools.rs` — `X402ChallengeInput.vc_ipfs_cid`

### 4.3 Policy List Sync

**Mechanism**: User whitelists/blacklists are stored as JSON, uploaded to IPFS, and the CID is recorded in the DID Document's `serviceEndpoint`:

```json
"service": [{
    "id": "did:ignite:z6Mk...#policy-list",
    "type": "IgnitePolicyList",
    "serviceEndpoint": "ipfs://<CID>"
}]
```

**Sync flow**:
```
1. MCP server updates local sled cache (add/remove merchant)
2. Calls list_store.upload_to_ipfs() to get new CID
3. Sends DIDComm list-sync-notification to phone App with new CID
4. Phone App can pull latest list via CID
```

**Code locations**:
- `ignite-pay-core/src/list_store.rs` — `sync_from_ipfs()`, `upload_to_ipfs()`
- `ignite-pay-mcp/src/main.rs` — IPFS upload logic after list changes
- `ignite-pay-core/src/didcomm.rs` — `build_list_sync_notification()`

### 4.4 Audit Log Backup

**Data pipeline**:
```
Transaction records → Merkle tree construction → protobuf serialization → Zstd compression → AES-256-GCM encryption → Upload IPFS LogChunk
                                                                                                                              ↓
                                                                                                        ChunkManifest tracks all chunk CIDs
```

**Backup**: `sync_to_ipfs()` uploads unsynced SQLite entries to IPFS.

**Restore**: `restore_from_ipfs()` restores all transactions from a single manifest CID:
```
1. Download manifest → get all chunk CIDs
2. Download each chunk → decrypt → verify hash chain integrity
3. Sort by nonce and return all entries
```

**Code locations**:
- `ignite-pay-core/src/log_sync.rs` — Complete IPFS log pipeline
- `ignite-pay-core/proto/audit.proto` — Protobuf schema (ChunkManifest, LogChunk)
- `ignite_pay_app/rust/src/api/log_store.rs` — Phone-side sync/restore bridge

### 4.5 IPFS Client Architecture

```rust
trait IpfsClient {
    async fn upload(&self, data: &[u8]) -> Result<String>;  // Returns CID
    async fn download(&self, cid: &str) -> Result<Vec<u8>>;  // Downloads by CID
}
```

| Implementation | Purpose | Status |
|---------------|---------|--------|
| `MockIpfsClient` | Development/testing (in-memory HashMap) | Default |
| `KuboIpfsClient` | Production (local Kubo node) | Enabled via `kubo` feature |

Current configuration defaults to `mode = "mock"`. Production deployments require `mode = "kubo"` with a local Kubo node.

---

## 5. Three-Layer Collaboration

### 5.1 Complete Data Flow

```
Merchant (local Ed25519 keypair)
    │
    │ Generate did:ignite:z...
    │
    ▼
did-registry (REST API)
    ├── Issue W3C VC (platform Ed25519 signature)    ──→ IPFS storage → CID
    ├── Sign VC binding: sign(subject_pk || vc_hash)
    ├── Obtain ZK validity proof from Photon RPC
    └── Build Light System Program CPI instructions
         │
         ▼
    Solana Blockchain
    ├── ignite-pay-did-program (Anchor + Light SDK)
    │       ├── initialize_did    — Compressed account creation
    │       ├── update_did_with_vc — Compressed account update
    │       └── revoke_vc         — VC revocation PDA
    │
    ├── Light Protocol State Trees (Merkle Tree)
    │       └── MerchantCompressedDid leaf nodes (hashes)
    │
    ├── PlatformConfig PDA (platform Ed25519 public key)
    └── RevokedVc PDAs (per-VC revocation entries)
```

### 5.2 Dual-Layer Payment Verification Model

**Layer 1 (Off-chain Fast Filtering)**:
1. Extract merchant VC from X402 response (inline or IPFS CID)
2. Verify VC signature with platform public key
3. Get Merkle Proof from the indexer
4. Locally verify `Proof + Leaf == Root`
5. Check `MerchantLeaf.status == 0 (active)`

**Layer 2 (On-chain Enforcement)**:
1. Agent calls the settlement contract
2. Contract uses `spl_account_compression::verify_leaf` to confirm merchant is platform-attested
3. Contract verifies Session Key validity
4. If any verification fails, the transaction rolls back

### 5.3 Responsibility Summary

| Component | Responsibility |
|-----------|---------------|
| **ZK Compression** | On-chain storage: merchant DID compressed accounts, Merkle proofs, on-chain verification |
| **IPFS** | Off-chain storage: complete VC data, policy lists, encrypted audit logs |
| **did:ignite** | Identity: local keypair generation, DIDComm V2 communication, VC subject binding |

The relationship: DID is the identity layer (generated locally), ZK Compression is the on-chain identity data layer (on-chain proof), and IPFS is the off-chain data layer (complete data references). During payment verification, the MCP server fetches the complete VC from IPFS to verify the signature, while simultaneously obtaining compressed proof from the Merkle tree to verify on-chain identity — both layers together ensure the merchant is trustworthy.

---

## 6. Related Code Index

| File | Content |
|------|---------|
| `ignite-pay-did-program/src/state.rs` | `MerchantCompressedDid` struct |
| `ignite-pay-did-program/src/lib.rs` | On-chain instructions (initialize_did, update_did, etc.) |
| `ignite-pay-core/src/identity.rs` | `did:ignite` DID method definition |
| `ignite-pay-core/src/solana_did.rs` | `SolanaDidBridge` bridge layer |
| `ignite-pay-core/src/ipfs.rs` | `IpfsClient` trait + Kubo/Mock implementations |
| `ignite-pay-core/src/vc.rs` | `resolve_vc_from_ipfs()` |
| `ignite-pay-core/src/list_store.rs` | Policy list IPFS sync |
| `ignite-pay-core/src/log_sync.rs` | Audit log IPFS pipeline |
| `ignite-pay-core/proto/audit.proto` | Audit log protobuf schema |
| `did-registry/src/handlers/register.rs` | DID registration + ZK proof acquisition |
| `did-registry/src/handlers/proof.rs` | ZK validity proof endpoint |
| `ignite-pay-solana/Cargo.toml` | Light SDK dependency |
| `ignite-pay-mcp/src/main.rs` | IPFS client init, VC resolution, list upload |
| `ignite-pay-mcp/config.toml` | IPFS configuration (mode, kubo_url) |
