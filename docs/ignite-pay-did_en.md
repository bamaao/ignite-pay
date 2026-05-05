**Implementation documentation for AI Agent payment identity based on `did:ignite` and DIDComm V2.** This document defines the specification for the `did:ignite` DID method in the Ignite Pay system, as well as the payment authorization flow based on DIDComm V2 encrypted communication.

---

# Implementation Documentation: Ignite Pay `did:ignite` DID System

## 1. Overview
### 1.1 Positioning
This implementation uses locally generated key pairs to create `did:ignite` decentralized identities. Through the DIDComm V2 protocol, encrypted communication channels are established between the MCP Server, Mediator, and the mobile app, enabling the authorization loop when an AI Agent encounters an HTTP 402 payment challenge.

### 1.2 Core Capabilities
* **Local Identity**: `did:ignite` is based on Ed25519 key pairs. No on-chain registration is required; identities can be used immediately after local generation.
* **Encrypted Communication**: End-to-end message confidentiality and authenticity are ensured through DIDComm V2 JWE (authcrypt).
* **Proxy Routing**: With DIDComm Mediator relay, asynchronous authorization between the Agent and the mobile app is supported.
* **Instant Payment**: Based on the X402 protocol to parse 402 responses, combined with amount threshold policies to enable automatic or interactive payment.
* **Trusted Endorsement**: Through a platform signature mechanism (VC Attestation), Verifiable Credentials are issued to legitimate merchants. MCP/Skill verifies merchant legitimacy when processing 402 responses.
* **Policy Cache**: User blacklists and whitelists are stored on IPFS. MCP/Skill pulls them at startup and stores them in a sled local cache for fast risk-control decisions.

---

## 2. `did:ignite` DID Method Specification

### 2.1 Identifier Format

```
did:ignite:z<multibase-base58btc>
```

- **Prefix**: `did:ignite:`
- **Multibase indicator**: `z` (denotes base58btc encoding)
- **Encoded content**: `0xed 0x01` (multicodec Ed25519 public key prefix) + 32-byte Ed25519 public key

Example: `did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK`

### 2.2 Key System

| Purpose | Algorithm | Key Size | DID Document Fragment ID |
| :--- | :--- | :--- | :--- |
| Signing/Verification | Ed25519 | 32 bytes | `#key-signing-1` |
| Key Agreement (Encryption) | X25519 | 32 bytes | `#key-agreement-1` |

### 2.3 DID Document Structure

```json
{
  "@context": ["https://www.w3.org/ns/did/v1"],
  "id": "did:ignite:z6Mk...",
  "verificationMethod": [{
    "id": "did:ignite:z6Mk...#key-signing-1",
    "type": "Ed25519VerificationKey2020",
    "controller": "did:ignite:z6Mk...",
    "publicKeyMultibase": "z<multibase-base58btc(0xed+0x01+ed25519_pubkey)>"
  }],
  "keyAgreement": [{
    "id": "did:ignite:z6Mk...#key-agreement-1",
    "type": "X25519KeyAgreementKey2020",
    "controller": "did:ignite:z6Mk...",
    "publicKeyBase64": "<base64-nopad(x25519_pubkey)>"
  }],
  "service": [{
    "id": "did:ignite:z6Mk...#policy-list",
    "type": "IgnitePolicyList",
    "serviceEndpoint": "ipfs://<CID>"
  }]
}
```

### 2.4 Identity Lifecycle

1. **Generation**: Call `generate_ignite_did()` to generate an Ed25519 key pair and derive the DID identifier from the public key.
2. **Registration**: Register keys through the DIDComm Agent for subsequent signing and encryption.
3. **Publishing**: Send the complete DID Document via `peer-introduction` during the Mediator handshake phase.
4. **Resolution**: The recipient extracts public keys from the DID Document via `parse_did_document()` and registers them as a communication peer.

---

## 3. System Architecture

### 3.1 Components and Communication Topology

```
AI Agent (Claude/etc)
  │  stdio (JSON-RPC 2.0 / MCP)
  ▼
ignite-pay-mcp (MCP Server)
  │  WebSocket (DIDComm JWE)
  ▼
didcomm-mediator
  │  DIDComm forward / pickup
  ▼
Phone (Flutter App)
```

### 3.2 Storage Architecture

| Layer | Technology | Stored Content | Persistence |
| :--- | :--- | :--- | :--- |
| **Identity Layer** | In-memory (DIDCommAgent) | `did:ignite` key pairs, peer public keys | Process lifecycle |
| **Payment Layer** | sled (embedded KV) | PaymentRequest records, status, transaction signatures | Persisted to disk |
| **Authorization Layer** | DashMap (in-memory) | PendingAuthStore (oneshot channel mapping) | Process lifecycle |
| **Policy Layer** | IPFS (decentralized storage) + sled (local cache) | Blacklists/whitelists, merchant VCs | IPFS persistent + sled local cache |
| **Trust Layer** | Platform DID (built-in) | Platform signing public key, VC verification logic | Released with version |

### 3.3 Configuration

Loaded via `config.toml` (or path specified by the `IGNITE_PAY_CONFIG` environment variable):

```toml
[mediator]
ws_url = "ws://127.0.0.1:8080/ws"
phone_did = ""

[storage]
path = "./data"

[policy]
auto_approve_max = 0      # 0 = disable auto-approval
auth_timeout = 300         # Authorization timeout (seconds)
```

---

## 4. Core Flow Specification

### 4.1 Mediator Connection Handshake

When the MCP Server starts, it connects to the Mediator via WebSocket and performs a three-step plaintext handshake:

| Step | Direction | Message Type | Description |
| :--- | :--- | :--- | :--- |
| 1 | Client -> Mediator | `coordinate-mediation/2.0/mediate-request` | Register as a Mediator client |
| 2 | Mediator -> Client | `coordinate-mediation/2.0/mediate-grant` | Mediator confirmation |
| 3 | Client -> Mediator | `coordinate-mediation/2.0/keylist-update` (add) | Register receiving key `{did}#key-1` |
| 4 | Mediator -> Client | `coordinate-mediation/2.0/keylist-update-response` | Confirm key registration |
| 5 | Client -> Mediator | `peer-did-discovery/1.0/discover` | Send complete DID Document |

After the handshake completes, the server enters the encrypted message receiving loop. If disconnected, it automatically reconnects every 3 seconds.

### 4.2 X402 Payment Challenge Processing

When an AI Agent encounters an HTTP 402 response, it calls the MCP Tool `process_x402_challenge`:

**Input**:
```json
{
  "challenge_body": "<402 response JSON>",
  "phone_did": "did:ignite:z..."
}
```

**402 Response Parsing**: Extract payment parameters from the first element of the `accepts` array:

| Field | Purpose | Default Value |
| :--- | :--- | :--- |
| `paymentType` | Payment type | `"transfer"` |
| `network` | Network | `"unknown"` |
| `token` | Token identifier | `"unknown"` |
| `amount` | Amount (smallest unit) | `0` |
| `recipient` | Recipient | `"unknown"` |

**X402 Extension Header Parsing** (defined in `ignite-pay-did-spl-account-compression.md` section 4.2):

| Field | Purpose | Default Value |
| :--- | :--- | :--- |
| `x402-merchant-did` | Merchant `did:ignite` identifier, used for blacklist/whitelist matching | Required |
| `x402-payment-address` | Merchant Solana receiving address | Required |
| `x402-merkle-context` | On-chain Merkle Tree address (optional, used for identity verification) | `null` |

**Decision Flow**:

```
Received 402
  │
  ├─ Extract merchant_did (from x402-merchant-did extension header)
  │
  ├─ Merchant legitimacy verification (see section 4.5 / 4.9)
  │    ├─ Find merchant VC (attached to 402 response or referenced via IPFS CID)
  │    ├─ Verify VC signature + validity period (platform public key verification)
  │    ├─ (On-chain layer) Obtain Merkle Proof, locally verify Proof + Leaf == Root
  │    │
  │    ├─ Verification failed → Block immediately, return rejection
  │    └─ Verification passed ↓
  │
  ├─ Blacklist hit (merchant_did is in the blacklist)?
  │    YES → Block immediately, return rejection
  │
  │    NO ↓
  │
  ├─ Whitelist hit + within limit (merchant_did is in the whitelist && amount <= list_max_amount)?
  │    YES → Auto-approve, execute mock payment, return tx signature
  │
  │    NO ↓
  │
  ├─ amount <= auto_approve_max && auto_approve_max > 0?
  │    YES → Execute mock payment, return tx signature
  │
  │    NO ↓
  │
  ├─ Create PaymentRequest (status: PendingAuth)
  ├─ Save to sled
  ├─ Build DIDComm authorization request message
  ├─ JWE encryption (authcrypt) → Send to mobile app via Mediator
  └─ Wait for mobile response (timeout: auth_timeout seconds)
       │
       ├─ true  → Mobile app has created on-chain Session Key
       │    ├─ Extract session_key_pubkey, chain_tx_signature from response
       │    ├─ Save SessionKeyInfo to sled
       │    ├─ Execute on-chain payment using Session Key (call ExecutePayment contract)
       │    └─ Status → Executed
       ├─ false → Status → Rejected
       └─ Timeout → Status → Expired
```

### 4.3 Authorization Messages

#### 4.3.1 Authorization Request (MCP Server -> Mobile App)

The DIDComm authorization message sent from MCP Server to the mobile app:

* **Message Type**: `https://didcomm.org/ignite-pay/1.0/payment-auth-request`
* **Encryption**: JWE authcrypt (via `pack_authcrypt`)
* **Message Body**:

| Field | Type | Description |
| :--- | :--- | :--- |
| `payment_id` | string | UUID |
| `merchant_did` | string | Recipient DID |
| `amount` | number | Amount |
| `description` | string | Human-readable description |

#### 4.3.2 Authorization Response (Mobile App -> MCP Server)

The DIDComm authorization response message returned from the mobile app to MCP Server:

* **Message Type**: `https://didcomm.org/ignite-pay/1.0/payment-auth-response`
* **Encryption**: JWE authcrypt
* **Message Body**:

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `payment_id` | string | Yes | Corresponding payment request UUID |
| `authorized` | bool | Yes | Whether this payment is authorized |
| `session_key_pubkey` | string | Required when authorized | Base58 public key of the on-chain Session Key (only present when `authorized=true`) |
| `session_key_tx_signature` | string | Required when authorized | On-chain transaction signature for Session Key registration (only present when `authorized=true`) |
| `session_expires_at` | number | Required when authorized | Session Key expiration time (Unix timestamp, only present when `authorized=true`) |
| `spending_limit` | number | Required when authorized | Session Key per-transaction/cumulative spending limit (lamports, only present when `authorized=true`) |
| `scopes` | string[] | Required when authorized | Session Key permission scopes (e.g. `["sol:transfer"]`, only present when `authorized=true`) |
| `list_action` | string | Yes | List operation: `"add_whitelist"` / `"add_blacklist"` / `"remove_whitelist"` / `"remove_blacklist"` / `"none"` |
| `list_label` | string | No | User-defined note (e.g. `"ShopX Marketplace"`), recommended when `list_action` is not `"none"` |
| `list_max_amount` | number | No | Auto-approval limit for this merchant (smallest unit), only effective for `"add_whitelist"` |

> **Session Key Creation Flow**: After the user taps "Authorize" on the mobile app, the Flutter App calls the Rust bridge to generate an Ed25519 temporary key pair, then submits an on-chain transaction to register the Session Key to the Solana contract (binding owner, spending_limit, scopes, expires_at). After the on-chain transaction is confirmed, session_key_pubkey and chain_tx_signature are returned to MCP/Skill via DIDComm encrypted response. MCP/Skill subsequently uses this Session Key to execute on-chain payments on behalf of the user, without requesting authorization again.

### 4.4 Payment Execution

After receiving the authorization response, MCP/Skill uses the Session Key created by the mobile app to execute on-chain payment:

**On-chain Payment Flow**:

```
MCP/Skill receives authorized=true + session_key_pubkey
  │
  ├─ 1. Verify Session Key on-chain status
  │    ├─ Query on-chain Session Key registration info
  │    ├─ Verify session_key_pubkey matches on-chain record
  │    ├─ Verify not expired (current_slot < session_expires_at)
  │    └─ Verify spending_limit >= payment amount
  │
  ├─ 2. Build ExecutePayment transaction
  │    ├─ Sign payment instruction with Session Key
  │    ├─ Attach merchant Merkle Proof (on-chain merchant identity verification)
  │    └─ Submit to Solana cluster
  │
  ├─ 3. Contract verification and execution
  │    ├─ Verify Session Key signature validity
  │    ├─ Verify Session Key not expired
  │    ├─ Verify spending_limit not exceeded
  │    ├─ Verify merchant Merkle Proof (SPL Account Compression)
  │    ├─ Execute SOL/SPL Token transfer
  │    └─ Update Session Key spent amount
  │
  └─ 4. Update PaymentRequest status
       ├─ Status → Executed
       └─ Save on-chain transaction signature
```

**V0.1 Phase (Current)**: Uses mock payment. Transaction signature format is `tx_mock_{payment_id}_{uuid_v4}`. Session Key is created locally as a simulation, with no on-chain transaction involved.

**V1.0 Phase**: Session Key is registered through the Solana on-chain contract. MCP/Skill uses the real on-chain Session Key to execute payments.

**Session Key Lifecycle**:

```
Creation (Mobile App)       Usage (MCP/Skill)          Expiration/Closure
─────────────              ──────────────              ──────────────
User authorizes →          MCP/Skill executes →        expires_at reached →
  Generate Ed25519           payment using               Session Key invalidated
  temporary key pair         Session Key to              Funds returned to owner
  Submit on-chain            sign on-chain               Or user actively closes
  registration               transactions                Session Key
  Return to MCP/Skill        Reuse until
                             limit/time exhausted
```

### 4.5 Platform Signing and Merchant Onboarding

To ensure that AI Agents only pay legitimate merchants, a platform endorsement signature mechanism is introduced. The platform acts as a trusted third party, issuing Verifiable Credentials (VCs) to approved merchants. MCP/Skill verifies merchant legitimacy when processing 402 responses.

**Complete Flow**:

```
Merchant Application          Platform Endorsement           Payment Verification
────────                     ────────                       ────────
Submit DID + Metadata  →     Platform review               MCP/Skill receives 402
                               │                             │
                               ├─ Approved → Issue VC       ├─ Extract merchant_did
                               │   (Platform private key     ├─ Find corresponding VC
                               │    signature)               ├─ Verify signature with
                               │                             │   built-in platform public key
                               └─ Rejected                   ├─ Check validity period
                                                             │
                                                             ├─ Verification passed → Enter decision flow
                                                             └─ Verification failed → Reject payment
```

**Detailed Steps**:

1. **Merchant Application**: The merchant submits a `did:ignite` identifier and service metadata (name, type, description, etc.) to the platform.
2. **Platform Endorsement**: After approval, the platform issues a VC signed with the platform's private key, containing claims such as merchant DID, validity period, and service type.
3. **X402 Carriage**: The service provider attaches the VC in the 402 response (either embedded directly or referenced via IPFS CID).
4. **MCP/Skill Verification**: Upon receiving a 402, MCP/Skill uses the built-in platform public key to verify the VC signature's authenticity and validity period, confirming the merchant's legitimacy.

**VC Structure Definition**:

```json
{
  "@context": ["https://www.w3.org/2018/credentials/v1"],
  "type": ["VerifiableCredential", "IgniteMerchantCredential"],
  "issuer": "did:ignite:z6Mk...<platform_did>",
  "issuanceDate": "2025-01-01T00:00:00Z",
  "credentialSubject": {
    "id": "did:ignite:z6Mk...<merchant_did>",
    "service_type": "api-service",
    "merchant_name": "Example API Service"
  },
  "expirationDate": "2026-01-01T00:00:00Z",
  "proof": {
    "type": "Ed25519Signature2020",
    "verificationMethod": "did:ignite:z6Mk...<platform_did>#key-signing-1",
    "proofPurpose": "assertionMethod",
    "proofValue": "<ed25519_signature_bytes_base58>"
  }
}
```

| VC Field | Type | Description |
| :--- | :--- | :--- |
| `issuer` | string | Platform DID, identifying the issuer |
| `credentialSubject.id` | string | Merchant DID, identifying the endorsed party |
| `credentialSubject.service_type` | string | Merchant service type (e.g. `"api-service"`, `"data-provider"`) |
| `credentialSubject.merchant_name` | string | Human-readable merchant name |
| `expirationDate` | string (ISO 8601) | VC expiration time |
| `proof.proofValue` | string | Ed25519 signature of VC content by the platform's private key |

### 4.6 IPFS Blacklist/Whitelist Management

Users can choose to add merchants to a blacklist or whitelist when authorizing on the mobile app. The lists are stored on IPFS (decentralized, tamper-proof), and the current list CID is recorded in the DID Document. MCP/Skill pulls the lists from IPFS at startup and stores them in a sled local cache for subsequent fast risk-control decisions.

**Complete Flow**:

```
Mobile App Authorization      List Sync                      Local Decision
────────                     ────────                       ────────
User selects list action  →  MCP/Skill receives          →  Received 402
(list_action)                 auth response                  │
                              │                              ├─ Query sled cache
                              ├─ Parse list_action            │
                              │                              ├─ Blacklist hit → Block
                              ├─ "add_whitelist"             │
                              │   Append to whitelist cache   ├─ Whitelist hit + within limit → Approve
                              │                              │
                              ├─ "add_blacklist"             └─ Otherwise → Route to mobile
                              │   Append to blacklist cache     authorization
                              │
                              ├─ Async upload to IPFS
                              │   (update CID)
                              │
                              └─ DIDComm V2 notify mobile of new CID
```

**Detailed Steps**:

1. **List Storage**: Blacklists and whitelists are stored as JSON files on IPFS. The user's DID Document records the current list CID through the `service` endpoint.
2. **Startup Pull**: When MCP/Skill starts, it reads the list CID from the DID Document, pulls the list data from IPFS, parses it, and stores it in the sled local cache.
3. **Local Decision**: When a 402 is received, the sled local cache is queried first:
   - Blacklist hit → Block immediately, return rejection
   - Whitelist hit + within limit → Auto-approve, execute payment
   - Otherwise → Route to mobile authorization flow

**List Structure Definition**:

```json
{
  "version": 1,
  "owner_did": "did:ignite:z6Mk...<user_did>",
  "updated_at": "2025-06-15T10:30:00Z",
  "whitelist": [
    {
      "did": "did:ignite:z6Mk...<merchant_did>",
      "label": "ShopX Marketplace",
      "max_amount": 1000000,
      "expires": "2026-06-15T00:00:00Z"
    }
  ],
  "blacklist": [
    {
      "did": "did:ignite:z6Mk...<merchant_did>",
      "label": "Suspicious API",
      "expires": null
    }
  ]
}
```

| List Entry Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `did` | string | Yes | Merchant `did:ignite` identifier |
| `label` | string | Yes | User-defined note (e.g. `"ShopX Marketplace"`) |
| `max_amount` | number | No | Auto-approval limit for this merchant (smallest unit), used for whitelist entries only |
| `expires` | string (ISO 8601) / null | No | List entry expiration time; `null` means never expires |

### 4.7 List Synchronization Flow

After the mobile app returns an authorization response (`payment-auth-response`), MCP/Skill needs to synchronize and update the blacklist/whitelist based on the `list_action` field in the response:

**Complete Flow**:

```
Mobile app returns payment-auth-response
  │
  ├─ Parse authorized field → Handle payment authorization result
  │
  ├─ Parse list_action field
  │    │
  │    ├─ "add_whitelist"
  │    │    ├─ Build whitelist entry: { did, label, max_amount, expires }
  │    │    ├─ Append to sled local cache (whitelist)
  │    │    ├─ Async upload: merge lists → JSON → IPFS → get new CID
  │    │    └─ DIDComm V2 notify mobile of new CID
  │    │
  │    ├─ "add_blacklist"
  │    │    ├─ Build blacklist entry: { did, label, expires }
  │    │    ├─ Append to sled local cache (blacklist)
  │    │    ├─ Async upload: merge lists → JSON → IPFS → get new CID
  │    │    └─ DIDComm V2 notify mobile of new CID
  │    │
  │    ├─ "remove_whitelist"
  │    │    ├─ Remove matching whitelist entry from sled local cache
  │    │    ├─ Async upload: merge lists → JSON → IPFS → get new CID
  │    │    └─ DIDComm V2 notify mobile of new CID
  │    │
  │    ├─ "remove_blacklist"
  │    │    ├─ Remove matching blacklist entry from sled local cache
  │    │    ├─ Async upload: merge lists → JSON → IPFS → get new CID
  │    │    └─ DIDComm V2 notify mobile of new CID
  │    │
  │    └─ "none"
  │         └─ No list operation, only process payment authorization result
  │
  └─ Flow complete
```

**Consistency and Fault Tolerance Guarantees**:

* **Write Order**: List updates use a "local-first, remote-second" strategy (write sled cache first, then async upload to IPFS). If the async upload fails, the sled cache still retains the latest data; on next startup, the latest CID is fetched from the DID Document to re-pull.
* **Expiration Cleanup**: MCP/Skill checks the `expires` field on every list query. Expired entries are treated as non-existent. List entries are not physically deleted, only logically skipped. During the next list upload, expired entries are filtered out.
* **CID Update**: IPFS CID updates follow the sequence "upload new CID → update DID Document service endpoint → notify mobile app," ensuring the mobile app can always obtain a valid CID through the DID Document at any time.

**Notification Message** (MCP/Skill -> Mobile App):

* **Message Type**: `https://didcomm.org/ignite-pay/1.0/list-sync-notification`
* **Message Body**:

| Field | Type | Description |
| :--- | :--- | :--- |
| `list_cid` | string | CID of the new list on IPFS |
| `action` | string | Action performed: `"add_whitelist"` / `"add_blacklist"` / `"remove_whitelist"` / `"remove_blacklist"` |
| `target_did` | string | Merchant DID that was operated on |
| `timestamp` | string | Sync timestamp (ISO 8601) |

### 4.9 Unified Merchant Verification Model

This section defines the collaborative relationship between VC verification (Section 4.5 of this document) and Merkle Proof verification (`ignite-pay-did-spl-account-compression.md` Section 3.3). These two are in an AND relationship; both must pass.

**Verification Layers**:

```
Received X402 payment request
  │
  ├─ 1. VC Signature Verification (this document Section 4.5)
  │    ├─ Extract merchant VC from 402 response (embedded directly or referenced via IPFS CID)
  │    ├─ Verify Ed25519Signature2020 proof using built-in platform public key
  │    ├─ Check VC expirationDate not expired
  │    └─ Failed → Reject payment (merchant has no platform endorsement)
  │
  ├─ 2. On-chain Merkle Proof Verification (compression document Section 3.3, first layer)
  │    ├─ Obtain merchant leaf node Merkle Proof from indexer
  │    ├─ Locally verify Proof + Leaf == Root
  │    ├─ Check MerchantLeaf.status == 0 (active)
  │    └─ Failed → Reject payment (merchant not on-chain or revoked)
  │
  ├─ 3. Consistency Check
  │    ├─ DID public key hash in VC credentialSubject.id == on-chain merchant_did_hash
  │    └─ Mismatch → Reject payment (identity mismatch)
  │
  └─ All passed → Enter decision flow (Section 4.2 blacklist/whitelist/auto-approval/mobile authorization)
```

**End-to-End Merchant Onboarding Flow** (connecting Section 4.5 of this document with compression document Section 3.1):

```
Merchant                Platform                       On-chain                Merchant/MCP/Skill
────                    ────────                       ────────                ──────────────
Generate did:ignite key pair
Generate Solana receiving key pair
Submit DID + metadata  →
                        Review approved
                        ├─ Issue VC (Ed25519Signature2020)
                        ├─ Compute MerchantLeaf:
                        │    merchant_did_hash = SHA-256(DID_pubkey)
                        │    active_pubkey = Solana receiving address
                        │    platform_vc_hash = SHA-256(canonical_json(VC))
                        │    status = 0 (active)
                        │
                        ├─ Call update_leaf on-chain  →  Leaf node inserted
                        │                                Indexer generates Proof
                        │
                        └─ Return VC to merchant  ←───────────────────────
                                                         Merchant attaches VC in 402 response
                                                         MCP/Skill receives 402:
                                                         1. Verify VC signature
                                                         2. Verify Merkle Proof
                                                         3. Consistency check
                                                         → After passing, enter decision flow
```

---

## 5. MCP Tool Interface

| Tool Name | Input | Output |
| :--- | :--- | :--- |
| `process_x402_challenge` | `challenge_body`, `phone_did` | Payment result + tx signature / error message |
| `check_authorization` | `payment_id` | Payment status, amount, time, tx signature |
| `get_payment_history` | `limit` (default 10) | Most recent N payment records |
| `get_identity` | (none) | Current `did:ignite`, Mediator connection status |

---

## 6. Mediator-Supported Protocols

| Protocol | Version | Message Types |
| :--- | :--- | :--- |
| Coordinate Mediation | 2.0 | `mediate-request`, `mediate-grant`, `keylist-update`, `keylist-update-response` |
| Routing | 2.0 | `forward` |
| Message Pickup | 3.0 | `status-request`, `status`, `batch-pickup`, `batch`, `live-delivery-request` |
| Peer DID Discovery | 1.0 | `discover` |

---

## 7. Security Design

* **End-to-End Encryption**: Authorization requests are encrypted via DIDComm JWE authcrypt. The Mediator cannot read message content and only performs routing and forwarding.
* **Key Isolation**: `did:ignite` key pairs are managed by DIDCommAgent. MCP Server accesses them through `Arc<Mutex<DIDCommAgent>>` under controlled access.
* **Timeout Protection**: Unauthorized payment requests automatically expire after `auth_timeout` seconds, preventing indefinite hanging.
* **Reconnection Mechanism**: After Mediator disconnection, automatic reconnection is attempted every 3 seconds with a full handshake re-execution.

---

## 8. Current Status and Evolution

| Phase | Content | Status |
| :--- | :--- | :--- |
| **V0.1** (Current) | `did:ignite` local identity + DIDComm V2 communication + Mock payment + MCP Server | ✅ Implemented |
| **V1.0** | Mobile DIDComm authorization link + Session Key on-chain registration + SPL Account Compression merchant on-chain + on-chain identity verification | Pending development |
| **V1.1** | VC merchant endorsement + IPFS blacklist/whitelist + mobile list management + sled local cache risk-control decisions + on-chain payment contract | Pending development |
| **V2.0** | Solana on-chain payment integration (Session Key driven) + multi-chain DID mapping | Pending development |

---

> **Implementation Note**: The current V0.1 phase focuses on validating the feasibility of the `did:ignite` identity model and DIDComm V2 encrypted communication. Payment execution is a mock implementation, and the mobile authorization callback is not yet integrated. Session Key will be introduced with on-chain registration in the V1.0 phase: when the mobile app authorizes, it creates an on-chain Session Key contract and returns it to MCP/Skill for subsequent on-chain payments. The core identity module (`identity.rs`) and DIDComm module (`didcomm.rs`) are shared between `ignite-pay-skill` and `ignite-pay-mcp`, providing a stable foundation for subsequent phases. The `SessionManager` in `ignite-pay-solana/src/session.rs` provides local management of Session Keys (creation, lookup, expiration check, spending limit tracking), which will be extended for on-chain registration in V1.0.
