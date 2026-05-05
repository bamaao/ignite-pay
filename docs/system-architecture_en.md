# Ignite Pay System Architecture and Implementation Document

This document describes the overall architecture, component interactions, data flows, API references, and security design of the Ignite Pay system.

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Component Inventory](#2-component-inventory)
3. [Core Library Modules](#3-core-library-modules)
4. [Data Flow Diagrams](#4-data-flow-diagrams)
5. [DIDComm Message Protocol](#5-didcomm-message-protocol)
6. [Storage Architecture](#6-storage-architecture)
7. [Identity System](#7-identity-system)
8. [Security Design](#8-security-design)
9. [API Reference](#9-api-reference)
10. [Push Notification Architecture](#10-push-notification-architecture)
11. [Compliance Configuration](#11-compliance-configuration)
12. [State Channel Program](#12-state-channel-program)
13. [Error Handling and Retry Strategy](#13-error-handling-and-retry-strategy)
14. [Monitoring and Observability](#14-monitoring-and-observability)
15. [Deployment Overview](#15-deployment-overview)
16. [Performance and Capacity Planning](#16-performance-and-capacity-planning)

---

## 1. Architecture Overview

The Ignite Pay system adopts a four-layer architecture:

```
┌───────────────────────────────────────────────────────────────────┐
│                          Application Layer                        │
│    Sentinel (User App)  │  Ignite Merchant (Merchant App)  │  AI Agent │
├───────────────────────────────────────────────────────────────────┤
│                        Communication Layer                        │
│         DIDComm V2 (JWE authcrypt)  │  Mediator (Relay Router)         │
├───────────────────────────────────────────────────────────────────┤
│                         Service Layer                             │
│  User MCP │ Merchant MCP │ Channel Service │ Hub Registry │ DID  │
├───────────────────────────────────────────────────────────────────┤
│                          On-chain Layer                           │
│    ignite-pay-program │ DID Program │ Session Key │ ZK Compression│
└───────────────────────────────────────────────────────────────────┘
```

### Global Data Flow

```
AI Agent ──X402──> MCP Server ──DIDComm JWE──> Mediator ──push──> Phone App
                                       │                                  │
                                       │                          FCM (overseas) / WebSocket (domestic)
                                       │                                  │
                                       │                         HTTPS Pull (message retrieval)
                                       │                                  │
                                  Payment Decision Engine <────────────── User approve/reject
                            (VC verification + on-chain DID verification + list + limits)
                                       │
                                       ↓
                          Session Key on-chain payment (SOL/SPL Token)
                                       │
                                       ↓
                                 Solana Blockchain
```

---

## 2. Component Inventory

### 2.1 Service Components

| Component | Binary/Directory | Port | Transport Protocol | Storage | Description |
|:----------|:-----------------|:-----|:-------------------|:--------|:------------|
| PostgreSQL | External dependency | 5432 | TCP | PostgreSQL | Hub Registry database |
| Hub Registry | `ignite-pay-hub-registry` | 3004 | HTTP | PostgreSQL | Hub registration and discovery service |
| DIDComm Router | `didcomm-router` | 8080 | HTTP + WS | sled | DIDComm message router/relay |
| DID Registry | `did-registry` | 8081 | HTTP | sled | DID registration service |
| Channel Hub | `ignite-pay-channel-service --config config-hub.toml` | 3003 | HTTP | sled | Hub routing node, supports multi-hop |
| Channel Provider | `ignite-pay-channel-service --config config-provider.toml` | 3002 | HTTP | sled | Merchant-side channel service |
| Channel User | `ignite-pay-channel-service --config config.toml` | 3001 | HTTP | sled | User-side channel service |
| User MCP | `ignite-pay-mcp` | stdio | MCP (JSON-RPC 2.0) | sled | User-side MCP agent |
| Merchant MCP | `ignite-pay-merchant-mcp` | stdio | MCP (JSON-RPC 2.0) | sled | Merchant-side MCP agent |

### 2.2 Mobile Applications

| Application | Directory | Platform | Storage | Description |
|:------------|:----------|:---------|:--------|:------------|
| Sentinel | User-side Flutter App | iOS / Android | sled + SQLite | User payment authorization guard |
| Ignite Merchant | Merchant-side Flutter App | iOS / Android | sled | Merchant payment collection tool |

### 2.3 On-chain Programs

| Program | Program ID | Framework | Description |
|:--------|:-----------|:----------|:------------|
| State Channel Program | `DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe` | Anchor 1.0.0 | Channel settlement/dispute handling |
| DID Program | Configured in `did-registry` | Anchor + Light SDK | ZK Compression compressed accounts |

### 2.4 Component Dependency Graph

```
                    ┌─────────────────┐
                    │   AI Agent      │
                    └────────┬────────┘
                             │ stdio (JSON-RPC)
                    ┌────────▼────────┐
                    │    User MCP     │
                    │    (stdio)      │
                    └──┬──────────┬───┘
                       │          │
              WS (:8080)          │ HTTP (:3003)
                       │          │
              ┌────────▼──┐  ┌───▼────────────┐
              │ Mediator  │  │  Channel Hub    │
              │ (:8080)   │  │  (:3003)        │
              │           │  └───┬────────┬────┘
              │           │      │        │
              └──┬───┬────┘      │        │
                 │   │           │        │
           WS/FCM│   │HTTPS      │        │
                 │   │           │        │
          ┌──────▼─┐ │     ┌────▼─────────▼───┐
          │Sentinel│ │     │  Hub Registry     │
          │(User   │ │     │  (:3004)          │
          │ App)   │ │     └────┬──────────────┘
          └────────┘ │          │ PostgreSQL
                     │     ┌────▼─────────┐
                     │     │ PostgreSQL   │
                     │     │ (:5432)      │
                     │     └──────────────┘
                     │
              ┌──────▼──────┐     ┌──────────────┐
              │ Merchant    │     │  Channel     │
              │ App         │     │  Provider    │
              │ (Merchant   │     │  (:3002)     │
              │  side)      │     └──────────────┘
              └──────┬──────┘
                     │
              ┌──────▼──────┐
              │ Merchant    │
              │ MCP (stdio) │
              └─────────────┘
```

---

## 3. Core Library Modules

### 3.1 ignite-pay-core

Core protocol library providing DID identity management, DIDComm communication, list management, VC issuance and verification capabilities.

| Module | Functionality |
|--------|---------------|
| `identity` | DID generation, DID Document construction, identity persistence, DID signature verification |
| `didcomm` | DIDComm message constructor (15 message types), JWE encryption/decryption, Agent creation |
| `types` | Shared types: PaymentRequest, MerchantListEntry, VerifiableCredential, RiskControlDecision |
| `list_store` | Whitelist/blacklist management (sled + IPFS sync), risk control decisions |
| `vc` | Verifiable Credential issuance and verification |
| `ipfs` | IPFS upload/download abstraction layer |
| `audit_merkle` | SHA-256 Merkle tree audit log |
| `log_crypto` / `log_chunk` / `log_sync` | E2EE audit log (encrypt -> Zstd compress -> IPFS sync) |
| `solana_did` | SolanaDidBridge: DID on-chain verification bridge layer (feature gate: `solana`) |

### 3.2 ignite-pay-state-channel

State channel protocol library providing core state channel capabilities including channel management, Merkle trees, HTLC, routing, and compliance.

| Module | File | Description |
|:-------|:-----|:------------|
| `channel` | `channel.rs` | ChannelManager — Channel lifecycle management, sled persistence |
| `merkle` | `merkle.rs` | MerkleTree — Sorted-pair hashing binary tree (matching on-chain program) |
| `types` | `types.rs` | UTXOLeaf (Standard/HTLC/Compliance), LeafUpdate, SignedState, ChannelMetadata |
| `signing` | `signing.rs` | Ed25519 signing/verification, message construction |
| `pipeline` | `pipeline.rs` | Pipeline — Batch LeafUpdate builder with automatic rollback |
| `htlc` | `htlc.rs` | HtlcManager — HTLC preimage/lifecycle management |
| `hub` | `hub.rs` | HubManager — Hub registration/metrics, sled persistence |
| `routing` | `routing.rs` | RouteService — DFS route discovery/scoring |
| `multihop` | `multihop.rs` | MultiHopManager — Multi-hop payments with decreasing timelock |
| `compliance` | `compliance.rs` | ComplianceManager — Spending limits/auditing |
| `error` | `error.rs` | StateChannelError unified error type |
| `helpers` | `helpers.rs` | Helper utility functions |

**Key Dependencies**:

```toml
[dependencies]
solana-program = "2"           # Solana core types (no OpenSSL dependency)
solana-pubkey = "2"            # Pubkey type
ed25519-dalek = "1"            # Ed25519 signatures
borsh = "1"                    # Serialization
serde = { version = "1", features = ["derive"] }
sled = "0.34"                  # Embedded database
anyhow = "1"                   # Error handling
rand = "0.7"                   # Random number generation
hex = "0.4"                    # Hex encoding/decoding
tracing = "0.1"                # Logging
```

### 3.3 ignite-pay-solana

Solana on-chain interaction library providing merchant identity verification, Session Key management, and on-chain payment capabilities.

```
ignite-pay-solana/
├── src/
│   ├── lib.rs              # Module declarations + re-export solana_sdk
│   ├── types.rs            # MerchantLeaf, SessionTokenData, PayMode, PaymentResult
│   ├── error.rs            # SolanaError unified error type
│   ├── compression.rs      # CompressionService: Merkle Tree operations
│   ├── indexer.rs          # IndexerClient: Helius DAS API queries
│   ├── session.rs          # SessionManager: Ephemeral key creation/persistence/verification
│   └── payment.rs          # IgnitePayClient: SOL/SPL Token actual transfers
```

**Core Types**:

- `MerchantLeaf`: On-chain merchant identity leaf node (merchant_did_hash, active_pubkey, platform_vc_hash, status)
- `SessionTokenData`: Session Key on-chain PDA data (owner, ephemeral_pubkey, expiry, scopes, spending_limit)
- `PayMode`: Payment mode enum (SelfFunded / Sponsored)
- `PaymentResult`: Payment execution result

---

## 4. Data Flow Diagrams

### 4.1 X402 Payment Authorization Flow

```
AI Agent              MCP Server           Mediator           User App (Sentinel)
   │                      │                    │                     │
   │  HTTP Request        │                    │                     │
   ├─────────────────────>│                    │                     │
   │                      │                    │                     │
   │  402 Payment Req     │                    │                     │
   │<─────────────────────┤                    │                     │
   │                      │                    │                     │
   │  process_x402        │                    │                     │
   ├─────────────────────>│                    │                     │
   │                      │                    │                     │
   │                      │  Merchant          │                     │
   │                      │  verification      │                     │
   │                      │  (VC + Merkle)     │                     │
   │                      │                    │                     │
   │                      │  List/limit check  │                     │
   │                      │                    │                     │
   │                      │  payment-auth-req  │                     │
   │                      │  (JWE encrypted)   │                     │
   │                      ├───────────────────>│                     │
   │                      │                    │  FCM/WS push        │
   │                      │                    ├────────────────────>│
   │                      │                    │                     │
   │                      │                    │  HTTPS Pull (JWE)   │
   │                      │                    │<────────────────────┤
   │                      │                    │                     │
   │                      │                    │  User review +      │
   │                      │                    │  create Session Key │
   │                      │                    │                     │
   │                      │                    │  auth-response      │
   │                      │                    │  (JWE encrypted)    │
   │                      │                    │<────────────────────┤
   │                      │                    │                     │
   │                      │  auth-response     │                     │
   │                      │<───────────────────┤                     │
   │                      │                    │                     │
   │                      │  Session Key       │                     │
   │                      │  payment           │                     │
   │                      │  (SOL/SPL Token)   │                     │
   │                      │                    │                     │
   │  Payment result +    │                    │                     │
   │  tx signature        │                    │                     │
   │<─────────────────────┤                    │                     │
```

### 4.2 State Channel Payment Flow

```
User App (A)                    Hub (B)                     Solana
    │                              │                           │
    │  LeafUpdate (Transfer)       │                           │
    ├─────────────────────────────>│                           │
    │                              │                           │
    │                              │  Update Merkle Tree       │
    │                              │  Create SignedState       │
    │                              │                           │
    │  CoSign Request              │                           │
    │<─────────────────────────────┤                           │
    │                              │                           │
    │  CoSign Response             │                           │
    ├─────────────────────────────>│                           │
    │                              │                           │
    │  Payment Result              │                           │
    │  (sequence, leaf_index)      │                           │
    │<─────────────────────────────┤                           │
    │                              │                           │
    │           ── On Channel Close ──                         │
    │                              │                           │
    │  Close Channel               │                           │
    ├─────────────────────────────>│                           │
    │                              │  Settle TX                │
    │                              ├──────────────────────────>│
    │                              │                           │
    │                              │  Settlement Confirmed     │
    │                              │<──────────────────────────┤
```

### 4.3 QR Code Payment Collection Flow

```
Merchant App            User App (Sentinel)         Hub                Mediator
   │                         │                       │                    │
   │  Generate QR code       │                       │                    │
   │  (ignite://pay?d=...)   │                       │                    │
   │                         │                       │                    │
   │     ───── QR scan ─────>│                       │                    │
   │                         │                       │                    │
   │                         │  Parse PaymentQrData  │                    │
   │                         │  Confirm payment      │                    │
   │                         │                       │                    │
   │                         │  POST /v1/channels/   │                    │
   │                         │  {id}/pay             │                    │
   │                         ├──────────────────────>│                    │
   │                         │                       │                    │
   │                         │  Payment Result       │                    │
   │                         │<──────────────────────┤                    │
   │                         │                       │                    │
   │                         │                       │  payment-confirm   │
   │                         │                       │  (JWE)             │
   │                         │                       ├───────────────────>│
   │                         │                       │                    │
   │                         │                       │  WS/FCM push       │
   │  <──────────────────────────────────────────────────────────────────┤
   │                         │                       │                    │
   │  Confirm order          │                       │                    │
   │  Voice announcement     │                       │                    │
```

### 4.4 Multi-hop Payment Flow

Multi-hop payments allow funds to reach the destination through multiple Hub relays, using decreasing timelocks and shared hash_locks to ensure atomicity.

**Key Constants**:

| Constant | Value | Description |
|:---------|:------|:------------|
| `HOP_MARGIN` | 1000 slots (~6.7 min) | Timelock difference between adjacent hops |
| `HTLC_SAFETY_MARGIN` | 1000 slots | HTLC safety margin |
| `min_timelock` | `challenge_duration + 3 * HOP_MARGIN` | Minimum timelock for a single hop |

**Timelock Decrease Formula**:

```
base_timelock = current_slot + min_timelock(challenge_duration) + (num_hops - 1) * HOP_MARGIN
hop[i].timelock = base_timelock - i * HOP_MARGIN
```

The first hop has the longest timelock, and the last hop has the shortest. If the last hop times out and refunds, each upstream hop still has a `HOP_MARGIN` window to complete the refund.

**Route Discovery**:

1. Hub registers itself with `RouteService` (fee, latency, liquidity, success rate)
2. Call `discover_routes(RouteRequest)` to perform DFS search for candidate routes
3. Route scoring formula: `score = 0.3 * fee_score + 0.3 * latency_score + 0.4 * reliability_score`
4. `select_best_route()` selects the highest-scoring route

**Fee Calculation**: Derived backwards from the last hop; each hop charges a routing fee on top of the downstream amount:

```
amounts[last] = destination_amount
amounts[i]    = amounts[i+1] + amounts[i+1] * fee_rate_bps[i] / 10000
```

**Full Sequence Diagram**:

```
Sender(A)           Hub_1(B)            Hub_2(C)            Receiver(D)         Solana
   │                   │                   │                   │                   │
   │  1. discover_routes                   │                   │                   │
   │──(RouteService)──>│                   │                   │                   │
   │  routes           │                   │                   │                   │
   │<──────────────────┤                   │                   │                   │
   │                   │                   │                   │                   │
   │  2. create_payment(preimage, hash_lock, hops_metadata)    │                   │
   │──(MultiHopManager)                                                      │                   │
   │  payment_id, status=Pending                                             │                   │
   │                   │                   │                   │                   │
   │  3. HTLC Lock: hop[0]                 │                   │                   │
   │  LeafUpdate(Standard→HTLC)            │                   │                   │
   ├──────────────────>│                   │                   │                   │
   │                   │  HTLC Lock: hop[1]│                   │                   │
   │                   │  LeafUpdate       │                   │                   │
   │                   ├──────────────────>│                   │                   │
   │                   │                   │  HTLC Lock: hop[2]│                   │
   │                   │                   │  LeafUpdate       │                   │
   │                   │                   ├──────────────────>│                   │
   │                   │                   │                   │                   │
   │  status = Locked  │                   │                   │                   │
   │                   │                   │                   │                   │
   │  ── Preimage Reveal Phase (reverse propagation) ──        │                   │
   │                   │                   │                   │                   │
   │                   │                   │  4. reveal_preimage                   │
   │                   │                   │<──────────────────┤                   │
   │                   │                   │  SHA-256(preimage)==hash_lock ✓       │
   │                   │                   │                   │                   │
   │  status = Resolving                   │                   │                   │
   │                   │                   │                   │                   │
   │                   │  5. resolve_hop[1]│                   │                   │
   │                   │<──────────────────┤                   │                   │
   │                   │                   │                   │                   │
   │  6. resolve_hop[0]│                   │                   │                   │
   │<──────────────────┤                   │                   │                   │
   │                   │                   │                   │                   │
   │  status = Completed                   │                   │                   │
   │                   │                   │                   │                   │
   │           ── Settlement Phase (each hop settles on-chain independently) ──    │
   │                   │                   │                   │                   │
   │  settle hop[0]    │                   │                   │                   │
   ├──────────────────>│  settle hop[1]    │                   │                   │
   │                   ├──────────────────>│  settle hop[2]    │                   │
   │                   │                   ├──────────────────>│                   │
   │                   │                   │                   │  Settle TX        │
   │                   │                   │                   ├──────────────────>│
   │                   │                   │                   │                   │
   │           ── Timeout Failure Path ──  │                   │                   │
   │                   │                   │                   │                   │
   │  check_expiry(current_slot)           │                   │                   │
   │  hop[i].timelock_slot < current_slot  │                   │                   │
   │  → status = Failed │                  │                   │                   │
   │                   │                   │                   │                   │
   │  Each hop HTLC: Expired → Refunded   │                   │                   │
```

**Multi-hop Payment State Machine**:

```
Pending → Locked → Resolving → Completed
                    │
                    └→ Failed (timeout: hop.timelock_slot < current_slot)
```

**HTTP API Endpoints** (Channel Service multi-hop handlers):

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/v1/multihop/payments` | POST | Create multi-hop payment |
| `/v1/multihop/payments/{id}/resolve` | POST | Resolve specified hop |
| `/v1/multihop/payments/{id}/relay` | POST | Hub relay resolution |
| `/v1/multihop/payments/{id}` | GET | Query payment status |
| `/v1/routing/hubs` | POST | Register Hub routing info |
| `/v1/routing/edges` | POST | Add channel edge |
| `/v1/routing/find` | POST | Find route |
| `/v1/routing/refresh` | POST | Refresh routing graph |

---

### 4.5 DIDComm Channel Creation Flow

```
User App            MCP Server          Hub Registry          Channel Hub
   │                    │                    │                    │
   │  GET /v1/hubs      │                    │                    │
   ├───────────────────>│───────────────────>│                    │
   │                    │                    │                    │
   │  Hub list          │                    │                    │
   │<───────────────────┤<───────────────────┤                    │
   │                    │                    │                    │
   │  Select Hub        │                    │                    │
   │  create-channel-   │                    │                    │
   │  request (JWE)     │                    │                    │
   ├───────────────────>│                    │                    │
   │                    │                    │                    │
   │                    │  POST /v1/channels/open                │
   │                    ├────────────────────────────────────────>│
   │                    │                    │                    │
   │                    │  channel_id, root  │                    │
   │                    │<────────────────────────────────────────┤
   │                    │                    │                    │
   │  create-channel-   │                    │                    │
   │  response (JWE)    │                    │                    │
   │<───────────────────┤                    │                    │
```

### 4.6 Hub Registration Discovery Flow

```
Channel Hub              Hub Registry              App
    │                        │                      │
    │  POST /v1/hubs         │                      │
    │  (Register self)       │                      │
    ├───────────────────────>│                      │
    │                        │                      │
    │  hub_id                │                      │
    │<───────────────────────┤                      │
    │                        │                      │
    │  PUT /v1/hubs/{id}/    │                      │
    │  metrics (every N sec) │                      │
    ├───────────────────────>│                      │
    │                        │                      │
    │                        │  GET /v1/hubs        │
    │                        │<─────────────────────┤
    │                        │                      │
    │                        │  Hub list            │
    │                        ├─────────────────────>│
```

---

## 5. DIDComm Message Protocol

### 5.1 Message Type Summary

| Message | Type URI | Direction | Purpose |
|---------|----------|-----------|---------|
| OOB Invitation | `https://didcomm.org/out-of-band/2.0/invitation` | MCP → User | QR pairing invitation |
| Connection Request | `https://didcomm.org/ignite-pay/1.0/connection-request` | User → MCP | Establish connection |
| Connection Response | `https://didcomm.org/ignite-pay/1.0/connection-response` | MCP → User | Connection confirmation |
| Payment Auth Request | `https://didcomm.org/ignite-pay/1.0/payment-auth-request` | MCP → User | Request payment authorization |
| Payment Auth Response | `https://didcomm.org/ignite-pay/1.0/payment-auth-response` | User → MCP | Authorization response (includes Session Key) |
| Channel Payment Request | `https://didcomm.org/ignite-pay/1.0/channel-payment-request` | App → MCP | State channel payment request |
| Channel Payment Confirm | `https://didcomm.org/ignite-pay/1.0/channel-payment-confirm` | Hub → Merchant | Payment confirmation push |
| List Sync Notification | `https://didcomm.org/ignite-pay/1.0/list-sync-notification` | MCP → User | Whitelist/blacklist update |
| Channel Create Request | `https://didcomm.org/ignite-pay/1.0/create-channel-request` | App → MCP | Create channel request |
| Channel Create Response | `https://didcomm.org/ignite-pay/1.0/create-channel-response` | MCP → App | Create channel response |
| Mediation | `https://didcomm.org/coordinate-mediation/2.0/*` | Bidirectional | Mediator protocol |
| WS Authentication | `https://didcomm.org/ignite-pay/1.0/ws-challenge-response` | Bidirectional | WS authentication challenge |
| Message Pickup | `https://didcomm.org/messagepickup/3.0/*` | Bidirectional | Message pickup protocol |

### 5.2 Payment Authorization Request Message Body

`payment-auth-request`:

| Field | Type | Description |
|:------|:-----|:------------|
| `payment_id` | string | UUID |
| `merchant_did` | string | Payee DID |
| `amount` | number | Amount (smallest unit) |
| `description` | string | Human-readable description |

### 5.3 Payment Authorization Response Message Body

`payment-auth-response`:

| Field | Type | Required | Description |
|:------|:-----|:---------|:------------|
| `payment_id` | string | Yes | Payment request UUID |
| `authorized` | bool | Yes | Whether authorized |
| `session_key_pubkey` | string | When authorized | Session Key Base58 public key |
| `session_key_tx_signature` | string | When authorized | Registration transaction signature |
| `session_expires_at` | number | When authorized | Session Key expiry time (Unix) |
| `spending_limit` | number | When authorized | Spending limit (lamports) |
| `scopes` | string[] | When authorized | Permission scope |
| `list_action` | string | Yes | List operation |
| `list_label` | string | No | Custom note |
| `list_max_amount` | number | No | Whitelist auto-approve limit |

### 5.4 List Sync Notification Message Body

`list-sync-notification`:

| Field | Type | Description |
|:------|:-----|:------------|
| `list_cid` | string | IPFS new list CID |
| `action` | string | Action performed |
| `target_did` | string | Target merchant DID |
| `timestamp` | string | Sync timestamp (ISO 8601) |

### 5.5 Channel Creation Message Body

`create-channel-request`:

```json
{
  "hub_endpoint": "http://hub:3003",
  "provider_pubkey": "Base58SolanaPubkey",
  "token_mint": "Base58MintAddress",
  "deposit": 1000000000,
  "tree_depth": 8
}
```

`create-channel-response`:

```json
{
  "channel_id": "hex_encoded_32_bytes",
  "sequence": 0,
  "current_root": "hex_encoded_root",
  "success": true
}
```

### 5.6 Mediator Supported Protocols

| Protocol | Version | Message Types |
|:---------|:--------|:--------------|
| Coordinate Mediation | 2.0 | `mediate-request`, `mediate-grant`, `keylist-update`, `keylist-update-response` |
| Routing | 2.0 | `forward` |
| Message Pickup | 3.0 | `status-request`, `status`, `batch-pickup`, `batch`, `live-delivery-request` |
| Peer DID Discovery | 1.0 | `discover` |

### 5.7 Message Encryption

- **Encryption method**: JWE authcrypt (DIDComm V2 standard)
- **Signing key**: Ed25519 (`#key-signing-1`)
- **Encryption key**: X25519 (`#key-agreement-1`)
- **Security**: Mediator cannot read message body, only performs routing and forwarding

---

## 6. Storage Architecture

| Service | Storage Technology | Data Content | Persistence Path |
|:--------|:-------------------|:-------------|:-----------------|
| Channel User (:3001) | sled | Channel data, Merkle tree, signatures | `./data/channel_user` |
| Channel Provider (:3002) | sled | Channel data, Merkle tree, signatures | `./data/channel_provider` |
| Channel Hub (:3003) | sled | Channel data, Merkle tree, signatures, Hub metrics | `./data/channel_hub` |
| Hub Registry (:3004) | PostgreSQL | Hub registration info, performance metrics | PostgreSQL database |
| DIDComm Router (:8080) | sled | Message queues, routing tables, known peers | `./data` |
| DID Registry (:8081) | sled | DID documents, VC records | sled |
| User MCP | sled | Payment requests, Session Keys, list cache | `./data` |
| Merchant MCP | sled | Merchant identity, orders, channels | `./data/merchant-mcp` |
| Sentinel (Flutter) | sled + SQLite | DID identity, Session Keys, policies, audit logs | Local storage |
| Ignite Merchant (Flutter) | sled | Key pairs, orders, channels, DIDComm identity | Local storage |

### Storage Tiers

| Tier | Implementation | Stored Content | Lifecycle |
|:-----|:---------------|:---------------|:----------|
| Identity Tier | Memory (DIDCommAgent) | `did:ignite` key pairs, peer public keys | Process lifecycle |
| Payment Tier | sled (embedded KV) | PaymentRequest records, state, transaction signatures | Persisted to disk |
| Authorization Tier | DashMap (memory) | PendingAuthStore (oneshot channel mapping) | Process lifecycle |
| Policy Tier | IPFS + sled (local cache) | Blacklist/whitelist, merchant VCs | IPFS persistent + sled cache |
| Trust Tier | Platform DID (built-in) | Platform signing public key, VC verification logic | Released with version |

---

## 7. Identity System

### 7.1 DID Method: `did:ignite`

**Identifier Format**:

```
did:ignite:z<multibase-base58btc>
```

- **Prefix**: `did:ignite:`
- **Multibase indicator**: `z` (base58btc encoding)
- **Encoded content**: `0xed 0x01` (multicodec Ed25519 public key prefix) + 32-byte Ed25519 public key

**Example**: `did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK`

### 7.2 Key System

| Purpose | Algorithm | Key Size | DID Document Fragment ID |
|:--------|:----------|:---------|:-------------------------|
| Signing/Verification | Ed25519 | 32 bytes | `#key-signing-1` |
| Key Agreement (Encryption) | X25519 | 32 bytes | `#key-agreement-1` |

### 7.3 DID Document Structure

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

### 7.4 Identity Lifecycle

1. **Generation**: Call `generate_ignite_did()` to generate an Ed25519 key pair and derive the DID identifier from the public key
2. **Registration**: Register keys through the DIDComm Agent for subsequent signing and encryption
3. **Publication**: Send the complete DID Document via `peer-introduction` during the Mediator handshake phase
4. **Resolution**: The recipient extracts public keys from the DID Document via `parse_did_document()` and registers as a communication peer

### 7.5 Merchant Dual-DID Architecture

The Merchant App manages two independent identities:

| Identity | DID Format | Purpose | Storage Location |
|----------|------------|---------|------------------|
| State Channel DID | `did:ignite:<raw_base58>` | QR code generation, channel operations, on-chain signing | sled `keypairs` tree |
| DIDComm Communication DID | `did:ignite:z<multicodec_base58>` | JWE encryption/decryption, Mediator message sending/receiving | sled `didcomm_identity` tree |

The two key systems are completely independent and do not interfere with each other.

### 7.6 Merchant DID Three-Layer Key Structure

| Key Type | Definition | Storage Location | Role |
|:---------|:-----------|:-----------------|:-----|
| Original Public Key (Root) | Solana address at merchant registration | Permanent on-chain ID | DID anchor, immutable |
| Controller Key | Pure Ed25519 key pair | Merchant local/offline | DID document modification authority |
| Recovery Key | Backup Ed25519 key pair | Offline cold storage | Reset when Controller Key is lost |

### 7.7 ZK Compression Identity

**On-chain Structure**:

- **Storage**: `MerchantLeaf` leaf nodes stored in Concurrent Merkle Tree
- **Tree parameters**: maxDepth=14, maxBufferSize=64 (supports ~16K merchants)
- **Leaf fields**:
  - `merchant_did`: SHA-256 hash
  - `active_pubkey`: Solana receiving public key
  - `platform_vc_hash`: Platform VC hash
  - `slot_updated`: Update slot
  - `status`: 0=active

**Two-Layer Verification**:

1. **Off-chain fast filtering**: Obtain Merkle Proof via Helius DAS API, verify locally with `verify_proof_locally()`
2. **On-chain mandatory verification**: Submit `verify_leaf` instruction to Solana, verified by the on-chain program

---

## 8. Security Design

### 8.1 Transport Security

| Security Measure | Implementation | Description |
|:-----------------|:---------------|:------------|
| End-to-end encryption | DIDComm V2 JWE authcrypt | Mediator cannot read message body |
| Dual-layer authentication | JWT (transport layer) + DIDComm signature (message layer) | Transport layer verifies "who is calling the API", message layer verifies "who sent the message" |
| Outer protection | TLS 1.3 | Protects outer metadata |
| Replay prevention | Check DIDComm Message `id` (Unique Message ID) | Prevents duplicate message submission |
| Expiry validation | Check `expires_time` | Discard stale instructions |

### 8.2 Session Key Risk Control

| Security Measure | Description |
|:-----------------|:------------|
| Expiry time check | `expires_at` field, Session Key becomes invalid after expiry |
| spending_limit | Single/cumulative spending limit, transactions exceeding limit are rejected |
| scopes | Permission scope restriction (`["sol:transfer", "spl:transfer"]`) |
| Prohibited instructions | Session Key cannot execute control instructions such as UpdateState / CloseAccount |
| CloseSession | In self-funded mode, remaining Gas can be refunded to the main wallet after expiry |

**Two Payment Modes**:

| Feature | Self-Funded Mode (SelfFunded) | Sponsored Mode (Sponsored) |
|:--------|:------------------------------|:---------------------------|
| Gas source | Ephemeral key account (pre-funded) | Project Relayer wallet |
| User perception | Requires one "funding" transaction confirmation | Zero perception |
| Degree of centralization | Fully decentralized | Depends on Relayer service |
| Use case | Large-amount, low-frequency settlement | High-frequency, micro-amount Agent automated payments |

### 8.3 HTLC Safety Margins

| Parameter | Default Value | Description |
|:----------|:--------------|:------------|
| default_challenge_duration | 5000 slots | Challenge duration |
| default_min_challenge_delay | 1000 slots | Minimum challenge delay |
| default_settle_window | 10000 slots | Settlement window |
| auto_close_offset | 500000 slots | Auto-close offset |
| default_tree_depth | 4 | Default Merkle tree depth (supports up to 12) |

### 8.4 Payment Decision Engine

Upon receiving an X402 pending payment request, the following 6-level priority checks are performed sequentially:

| Priority | Scenario | Condition | Action |
|:---------|:----------|:----------|:-------|
| 1 | VC verification failed | Attached VC signature invalid/expired/issuer mismatch | Reject payment, return verification failure reason |
| 2 | On-chain DID verification failed | Merchant DID not registered in Merkle Tree | Reject payment, return "merchant not found on-chain" |
| 3 | Blacklist block | `provider_did` is on blacklist | Immediately abort, return `Security Risk: Provider Blocked` |
| 4 | Whitelist auto-approve | `provider_did` is on whitelist && amount <= `max_amount` | Execute on-chain payment directly |
| 5 | Global threshold auto-approve | Amount <= `auto_approve_max` && `auto_approve_max > 0` | Automatically execute on-chain payment |
| 6 | Interactive authorization | None of the above satisfied | Trigger DIDComm push authorization request to user's phone |

### 8.5 Merchant Verification Flow

```
Received X402 pending payment request
  │
  ├─ 1. VC signature verification
  │    ├─ Extract merchant VC from 402 response
  │    ├─ Verify Ed25519Signature2020 proof using built-in platform public key
  │    ├─ Check VC expirationDate not expired
  │    └─ Failed → Reject payment
  │
  ├─ 2. On-chain Merkle Proof verification
  │    ├─ Obtain merchant leaf node Merkle Proof from indexer
  │    ├─ Local verification: Proof + Leaf == Root
  │    ├─ Check MerchantLeaf.status == 0 (active)
  │    └─ Failed → Reject payment
  │
  ├─ 3. Consistency check
  │    ├─ DID public key hash of credentialSubject.id in VC == on-chain merchant_did_hash
  │    └─ Inconsistent → reject payment
  │
  └─ All passed → enter decision flow
```

---

## 9. API Reference

### 9.1 Hub REST API (:3003)

| Endpoint | Method | Purpose |
|------|------|------|
| `/v1/channels/open` | POST | Open state channel |
| `/v1/channels/{id}/pay` | POST | Channel payment |
| `/v1/channels/{id}/close` | POST | Co-operative close channel |
| `/v1/channels/{id}/settle` | POST | Initiate settlement |
| `/v1/channels/{id}/claim` | POST | Claim leaf |
| `/v1/channels/{id}/finalize` | POST | Finalize settlement |

### 9.2 Hub Registry REST API (:3004)

| Endpoint | Method | Purpose |
|------|------|------|
| `/v1/hubs` | POST | Register Hub |
| `/v1/hubs` | GET | List Hubs (supports status, token_mint, limit, offset parameters) |
| `/v1/hubs/{hub_id}` | GET | Get Hub details |
| `/v1/hubs/{hub_id}` | PUT | Update Hub |
| `/v1/hubs/{hub_id}` | DELETE | Deregister Hub (set to inactive) |
| `/v1/hubs/{hub_id}/metrics` | GET | Get Hub performance metrics |
| `/v1/hubs/{hub_id}/metrics` | PUT | Update Hub performance metrics |

### 9.3 Mediator REST API (:8080)

| Endpoint | Method | Purpose |
|------|------|------|
| `/v1/auth/challenge` | GET | Get authentication nonce |
| `/v1/auth/token` | POST | Exchange signature for JWT |
| `/v1/sync/list` | GET | Pull message list (cursor-based pagination) |
| `/v1/sync/messages/{id}` | GET | Get single message |
| `/v1/agents/{id}/command` | POST | Send encrypted command |
| `/v1/agents/bind` | POST | Bind Agent DID |
| `/v1/devices/register-token` | POST | Register push channel (FCM token or websocket) |

### 9.4 DID Registry REST API (:8081)

| Endpoint | Method | Purpose |
|------|------|------|
| `/health` | GET | Health check |
| `/v1/did/resolve/{did}` | GET | Resolve DID Document |
| `/v1/auth/nonce` | GET | Get authentication nonce |
| `/v1/merchants/register` | POST | Register merchant (on-chain ZK Compression) |
| `/v1/merchants/confirm` | POST | Confirm merchant registration |
| `/v1/merchants/verify/{did}` | GET | Verify merchant identity |
| `/v1/merchants/status/{did}` | GET | Query merchant status |
| `/v1/merchants/rotate-key` | POST | Rotate merchant key |
| `/v1/merchants/update-vc` | POST | Update merchant VC |
| `/v1/vc/issue` | POST | Issue VC |
| `/v1/vc/revoke` | POST | Revoke VC |
| `/v1/proof` | POST | Get ZK Compression Proof (public, no auth required) |
| `/v1/fees` | GET | List fee records |

**Fees**: register=5000, update_vc=2000, rotate_key=2000 lamports

### 9.5 User MCP Tool Interface

| Tool Name | Input | Output |
|:-------|:-----|:-----|
| `process_x402_challenge` | `challenge_body`, `phone_did` | Payment result + tx signature / error |
| `check_authorization` | `payment_id` | Payment status, amount, time, tx signature |
| `get_payment_history` | `limit` (default 10) | Most recent N payment records |
| `get_identity` | (none) | Current `did:ignite`, Mediator connection status |

### 9.6 Merchant MCP Tool Interface

| Tool Name | Required Params | Optional Params | Output |
|:-------|:---------|:---------|:-----|
| `generate_payment_qr` | `amount` | `description`, `order_id` | QR text (`ignite://pay?d=...`) + ASCII QR code |
| `check_payment` | `order_id` | — | Order status, amount, channel ID, confirmation time |
| `get_payment_history` | — | `limit` (default 20) | Most recent N order list |
| `get_channel_status` | — | `channel_id` | Single channel details or all channel list |
| `open_channel_with_hub` | `hub_endpoint` | `deposit` (default 0), `tree_depth` (default 8) | Prompt message (merchant-side channel opening initiated by user) |
| `close_channel` | `channel_id` | — | Close confirmation |
| `settle_channel` | `channel_id` | — | claim + finalize result |
| `get_identity` | (none) | — | Merchant DID, Hub connection status |

---

## 10. Push Notification Architecture

```
                    ┌───────────────────────┐
                    │      Mediator         │
                    │  (message relay + push)│
                    └─────┬──────────┬──────┘
                          │          │
              ┌───────────┘          └───────────┐
              │                                  │
        zh_CN users                        Non-zh_CN users
              │                                  │
     WebSocket long connection            FCM push notification
     (receive JWE directly)               (SIGNAL → pull JWE)
              │                                  │
              └──────────┬───────────────────────┘
                         │
                    pull_messages()
                    decrypt_message()
                    Confirm order / Authorize payment
```

| User Region | Push Method | Uplink Path | Downlink Path |
|:---------|:---------|:---------|:---------|
| Overseas | FCM signal + HTTPS pull | MCP → WS → Mediator → FCM Signal → App HTTPS pull | App → HTTPS → Mediator → WS → MCP |
| Mainland China | WebSocket direct push | MCP → WS → Mediator → WS direct push → App | App → HTTPS → Mediator → WS → MCP |

**Common behaviors**:
- On first connection: authenticate → pull offline messages
- After WS disconnection: pull offline messages first, then reconnect (3-second delay)
- When App returns to foreground: trigger `GET /v1/sync/list` as a catch-up sync

---

## 11. Compliance Configuration

Channel User (:3001) and Channel Hub (:3003) support compliance configuration:

```toml
[compliance]
spending_threshold = 1000000000    # Spending threshold: 1 SOL
per_channel_limit = 100000000      # Per-channel limit: 0.1 SOL
window_slots = 100000              # Sliding window: ~100000 slots (~1-2 days)
travel_rule_threshold = 500000000  # Travel Rule threshold: 0.5 SOL
```

The compliance module (`ComplianceManager`) provides:
- **Sliding window spending threshold**: Tracks total spending within a specified slot window
- **Per-channel limit**: Caps the maximum payment amount for a single channel
- **Travel Rule data**: Records identity information of both parties when exceeding the threshold

---

## 12. State Channel Program

### 12.1 On-chain Program

- **Program ID**: `DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe`
- **Framework**: Anchor 1.0.0
- **PDA Accounts**:
  - `channel`: Channel state account
  - `escrow`: Escrow account

### 12.2 UTXO Leaf Types

| Type | Description |
|:-----|:-----|
| Standard | Standard transfer leaf |
| HTLC | Hash Time-Locked leaf (conditional payment) |
| Compliance | Compliance marker leaf |

### 12.3 Configuration File Reference

**Channel User (`config.toml`)**:

```toml
[server]
host = "0.0.0.0"
port = 3001

[solana]
rpc_url = "https://api.devnet.solana.com"
channel_program_id = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe"
keypair_path = "./keys/user.key"

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "./data/channel_user"

[compliance]
spending_threshold = 1000000000
per_channel_limit = 100000000
window_slots = 100000
travel_rule_threshold = 500000000
```

**Channel Hub (`config-hub.toml`)**:

```toml
[server]
host = "0.0.0.0"
port = 3003

[solana]
rpc_url = "https://api.devnet.solana.com"
channel_program_id = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe"
keypair_path = "./keys/hub.key"

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "./data/channel_hub"

[compliance]
spending_threshold = 1000000000
per_channel_limit = 100000000
window_slots = 100000
travel_rule_threshold = 500000000
```

**DIDComm Router (`didcomm-router/config.toml`)**:

```toml
[server]
host = "0.0.0.0"
port = 8080

[router]
did = "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK"
max_queued_messages = 1000
max_message_age_seconds = 86400

[storage]
path = "./data"
```

**User MCP (`ignite-pay-mcp/config.toml`)**:

```toml
[mediator]
ws_url = "ws://127.0.0.1:8080/ws"
phone_did = ""

[storage]
path = "./data"

[policy]
auto_approve_max = 0
auth_timeout = 300

[platform]
did = "did:ignite:zPlatformDIDPlaceholder"
verifying_key_b64 = ""

[ipfs]
mode = "mock"

[solana]
rpc_url = "https://api.devnet.solana.com"
tree_address = ""
tree_authority = ""
das_endpoint = ""
pay_mode = "self_funded"
default_owner = ""
tree_authority_keypair_b58 = ""
```

**Merchant MCP (`ignite-pay-merchant-mcp/config.toml`)**:

```toml
[merchant]
did = ""
hub_endpoint = "http://localhost:3003"
hub_ws_url = "ws://localhost:3003/ws"

[mediator]
ws_url = "ws://localhost:4000/ws"

[storage]
path = "./data/merchant-mcp"

[solana]
rpc_url = "https://api.devnet.solana.com"
program_id = ""

[hub]
token_mint = ""
provider_pubkey = ""
```

---

## 13. Error Handling and Retry Strategy

### 13.1 Error Type System

Each service uses `thiserror` to define a unified error enum, mapped to HTTP status codes via Axum `IntoResponse`.

**DIDComm Router** (`didcomm-router/src/error.rs`):

| Error Variant | HTTP Status | Description |
|:---------|:-----------|:-----|
| `Unauthorized` | 401 | JWT/DIDComm signature verification failed |
| `SessionNotFound` | 404 | WebSocket session not found |
| `Didcomm` / `DidResolution` / `Storage` / `Protocol` | 500 | Internal error |

**DID Registry** (`did-registry/src/error.rs`):

| Error Variant | HTTP Status | Description |
|:---------|:-----------|:-----|
| `BadRequest` | 400 | Invalid request parameters |
| `Unauthorized` | 401 | JWT verification failed |
| `MerchantNotFound` | 404 | Merchant not found |
| `ProofVerificationFailed` | 500 | ZK Proof verification failed |
| Other | 500 | On-chain/storage/serialization error |

**Channel Service** (`ignite-pay-channel-service/src/error.rs`):

| Error Variant | HTTP Status | Description |
|:---------|:-----------|:-----|
| `BadRequest` | 400 | Invalid request parameters |
| `Unauthorized` | 401 | Signature verification failed |
| `ChannelNotFound` | 404 | Channel not found |
| `ComplianceHold` | 403 | Compliance freeze |
| `StateChannel` | 422 | State channel protocol error (signature/sequence/Merkle etc.) |
| `PeerUnreachable` | 502 | Peer unreachable |
| `OnChain` / `SolanaRpc` / `Storage` / `Internal` | 500 | Internal error |

**Hub Registry** (`ignite-pay-hub-registry/src/error.rs`):

| Error Variant | HTTP Status | Description |
|:---------|:-----------|:-----|
| `BadRequest` | 400 | Invalid parameters |
| `NotFound` | 404 | Hub not found |
| `Database` / `Internal` | 500 | Database/internal error |

**State Channel Library** (`ignite-pay-state-channel/src/error.rs`):

| Error Variant | Description |
|:---------|:-----|
| `InvalidSequence { expected, actual }` | Non-contiguous sequence number |
| `PrevHashMismatch` | Previous leaf hash mismatch |
| `InvalidSignature` | Ed25519 signature verification failed |
| `InsufficientBalance { required, available }` | Insufficient balance |

**VC Verification** (`ignite-pay-core/src/vc.rs`):

| Error Variant | Description |
|:---------|:-----|
| `InvalidSignature` | Invalid VC Ed25519 signature |
| `Expired` | VC has expired |
| `IssuerMismatch` | Issuer mismatch |
| `MissingProof` | Missing proof field |

### 13.2 WebSocket Reconnection Strategy

All components requiring persistent WebSocket connections (User MCP, Merchant MCP, User App, Skill) use the same reconnection pattern:

```
loop {
    match connect_and_run(...).await {
        Ok(()) => warn!("Mediator disconnected, reconnecting..."),
        Err(e) => error!("WS error: {}, reconnecting in 3s...", e),
    }
    tokio::time::sleep(Duration::from_secs(3)).await;
}
```

| Feature | Current Implementation |
|:-----|:---------|
| Reconnect interval | Fixed 3 seconds |
| Max retries | Unlimited |
| Backoff strategy | None (fixed interval) |
| Jitter | None |
| Affected components | User MCP, Merchant MCP, User App (Flutter/Rust), Skill |

### 13.3 Timeout Configuration

| Scenario | Timeout | Component | Location |
|:-----|:---------|:-----|:-----|
| WS auth challenge response | 10s | DIDComm Router (server) | `transport/ws.rs` |
| WS auth result | 5s | User MCP / User App | `mediator.rs` / `ws_client.rs` |
| WS message status query | 5s | User MCP | `mediator.rs` |
| WS message batch pickup | 10s | User MCP | `mediator.rs` |
| Payment authorization wait | Configurable (default 300s) | User MCP / Skill | `PolicyConfig.auth_timeout` |
| Nginx WS idle timeout | 3600s (1h) | Nginx reverse proxy | `nginx.conf` |
| HTTP client | No timeout | All services (reqwest default) | — |

### 13.4 Degradation and Fault Tolerance Strategy

**Message Delivery Degradation Chain**:

```
WebSocket real-time delivery
    │ Failure or user offline
    ▼
Message persistence queue (sled) → Pull on next user online
    │ FCM configured
    ▼
FCM push notification → App receives signal then HTTPS pull messages
    │ FCM not configured
    ▼
NoopNotificationSender (silent, messages still retained in queue)
```

**JWE Unwrapping Degradation**:

```
JWE authcrypt decryption
    │ Failure
    ▼
Plaintext JSON parsing (compatible with unencrypted messages)
```

**Session Key Degradation**:

```
Use Session Key from this authorization response
    │ Parse failure
    ▼
Use locally cached active Session Key
```

**Channel State Persistence Fault Tolerance**:

On local state update failure, silently ignore (`let _ = persist_state()`), not blocking the successful return of remote operations.

### 13.5 Known Limitations

| Limitation | Description |
|:-----|:-----|
| No exponential backoff | All WS reconnections use a fixed 3s interval; sustained failures may generate heavy reconnection traffic |
| No circuit breaker | No circuit breaker protection for external dependencies like Solana RPC, Hub |
| No rate limiting | API rate limiting not implemented |
| No graceful shutdown | All services do not handle SIGTERM/SIGINT; tokio tasks are forcefully terminated |
| HTTP client has no timeout | All `reqwest::Client::new()` use defaults (no timeout) |
| MCP services use anyhow | User MCP and Merchant MCP do not use structured error types |

---

## 14. Monitoring and Observability

### 14.1 Health Check Endpoints

| Service | Endpoint | Response Format | Check Depth |
|:-----|:-----|:---------|:---------|
| DIDComm Router (:8080) | `GET /health` | `"ok"` (plain text) | HTTP liveness only |
| DID Registry (:8081) | `GET /health` | `"ok"` (plain text) | HTTP liveness only |
| Channel User (:3001) | `GET /health` | `{"status":"ok"}` (JSON) | HTTP liveness only |
| Channel Provider (:3002) | `GET /health` | `{"status":"ok"}` (JSON) | HTTP liveness only |
| Channel Hub (:3003) | `GET /health` | `{"status":"ok"}` (JSON) | HTTP liveness only |
| Hub Registry (:3004) | No `/health` endpoint | — | — |

> All health check endpoints only verify that the HTTP service is responding; they do not check database connections, Solana RPC reachability, or sled status.

### 14.2 Logging System

**Unified framework**: All services use `tracing` + `tracing-subscriber` (with `env-filter` feature).

**Log configuration method**: Controlled via `RUST_LOG` environment variable.

```bash
# Global level
RUST_LOG=info ./target/release/channel-hub ./config-hub.toml

# Per-module filter
RUST_LOG=ignite_pay_channel_service=debug,ignite_pay_state_channel=trace ./channel-hub ./config-hub.toml
```

| Service | Default Filter | Output Target | File Logging |
|:-----|:---------|:---------|:---------|
| DIDComm Router | `didcomm_router=info` | stderr + `logs/router.log` (daily rotation) | Yes |
| DID Registry | `did_registry=info` | stderr | No |
| Channel User / Provider / Hub | `info` | stderr | No |
| Hub Registry | `info` | stderr | No |
| User MCP | `ignite_pay_mcp=info` | stderr + optional audit log file | Conditional (`AUDIT_LOG_DIR` env var) |
| Merchant MCP | `info` | stderr + optional audit log file | Conditional (`AUDIT_LOG_DIR` env var) |

**Enabling MCP Audit Logging**:

```bash
AUDIT_LOG_DIR=/var/log/ignite-pay ./target/release/ignite-pay-mcp ./config.toml
# Log file: /var/log/ignite-pay/ignite-pay-mcp.log (daily rotation)
```

**Key Log Points**:

| Service | Log Event | Level |
|:-----|:---------|:-----|
| DIDComm Router | WS connection auth success/failure | info / warn |
| DIDComm Router | HTTP DIDComm message received (incl. byte count) | info |
| DIDComm Router | WS session deregistration | info |
| DIDComm Router | FCM push failure | warn |
| Channel Service | LeafUpdate / CoSignRequest / HtlcPreimage | info |
| Channel Service | Multi-hop payment initialization | info |
| Hub Registry | Hub registration, metrics update | info |
| MCP Services | Mediator disconnect/reconnect | warn / error |
| MCP Services | Queued message count | info |

### 14.3 Hub Performance Metrics System

The system implements a domain-level metrics collection for routing scoring rather than general monitoring.

**HubMetrics Structure** (`ignite-pay-state-channel/src/hub.rs`):

| Field | Type | Description |
|:-----|:-----|:-----|
| `online_rate` | u16 (basis points) | Online rate (10000 = 100%) |
| `success_rate` | u16 (basis points) | Transaction success rate |
| `avg_latency_ms` | u32 | Average latency (milliseconds) |
| `total_routed` | u64 | Cumulative routed transactions |
| `total_transactions` | u64 | Cumulative total transactions |
| `active_channels` | u32 | Active channel count |
| `available_liquidity` | u64 | Available liquidity |
| `fee_rate_bps` | u16 | Fee rate (basis points) |

**Metrics Flow**:

```
Channel Hub ──(PUT /v1/hubs/{id}/metrics, periodic push)──> Hub Registry
                                                          │
RouteService ──(GET /v1/hubs/{id}/metrics)───────────────>│
    │                                                     │
    └─ score_route_from_metrics() used for routing score  │
```

**Metrics Integrity**: `compute_metrics_hash()` applies SHA-256 hash to all fields, usable for on-chain verification of metric authenticity.

### 14.4 Diagnostic Endpoints

| Endpoint | Service | Description |
|:-----|:-----|:-----|
| `GET /v1/merchants/status/{did}` | DID Registry | Merchant on-chain status query |
| `GET /v1/hub/info` | Channel Hub | Current Hub registration info and status |
| `GET /v1/compliance/{channel_id}` | Channel User | Channel compliance status |
| `GET /v1/hubs/{id}/metrics` | Hub Registry | Hub performance metrics query |

### 14.5 Known Limitations

| Limitation | Description |
|:-----|:-----|
| No Prometheus/OpenMetrics | No `/metrics` endpoint exposed; cannot integrate with Prometheus ecosystem |
| No distributed tracing | No `#[tracing::instrument]` or spans; logs are flat events |
| No request correlation ID | Cross-service requests have no correlation ID linking |
| No WebSocket heartbeat | No application-level ping-pong; relies on Nginx 1-hour timeout |
| Health checks have no dependency detection | `/health` does not verify sled/PostgreSQL/Solana RPC connectivity |
| Hub Registry has no `/health` | Only service missing a health check endpoint |

---

## 15. Deployment Overview

> For full deployment steps, detailed configuration, Docker orchestration, and troubleshooting, refer to the [Deployment Guide](deploy/system-deployment.md).

### 15.1 Startup Sequence

Services have inter-dependencies and must be started in the following order:

```
1. PostgreSQL          ← External dependency, Hub Registry database
       │
2. DIDComm Router      ← No external dependencies
   DID Registry        ← Depends on Solana RPC + on-chain program
       │
3. Channel User        ← Depends on Solana RPC
   Channel Provider    ← Depends on Solana RPC
   Channel Hub         ← Depends on Solana RPC + Hub Registry
       │
4. Hub Registry        ← Depends on PostgreSQL (schema auto-initializes)
       │
5. User MCP            ← Depends on DIDComm Router (WS) + Channel User (HTTP)
   Merchant MCP        ← Depends on DIDComm Router (WS) + Channel Hub (HTTP+WS)
       │
6. Mobile App          ← Depends on DIDComm Router + MCP
```

### 15.2 Environment Variables

| Variable | Applicable Services | Description |
|:-----|:---------|:-----|
| `RUST_LOG` | All services | Log filter level (default `info`) |
| `AUDIT_LOG_DIR` | User MCP, Merchant MCP | Audit log file directory (enables file logging when set) |

### 15.3 Key Management Summary

| Key Type | Format | Generation Method | Security Requirements |
|:---------|:-----|:---------|:---------|
| Solana Keypair | JSON array (64 bytes) | `solana-keygen new` | `chmod 400`, use HSM/KMS in production |
| Platform Signing Key | 32 bytes raw binary | `openssl rand -out file 32` | Offline backup, `chmod 400` |
| DID Identity | Ed25519 keypair | `ignite-pay-core::identity` | sled persistence |
| FCM Service Account | JSON | Firebase Console | Only needed by DIDComm Router |

---

## 16. Performance and Capacity Planning

### 16.1 Capacity Limits Overview

| Dimension | Limit | Source | Description |
|:-----|:-------|:-----|:-----|
| Message queue (per user) | 1000 messages | `max_queued_messages` config | FIFO evicts oldest messages |
| Message TTL | 86400s (24h) | `max_message_age_seconds` config | Expired messages silently discarded |
| Message sync pagination | Default 100, hard cap 1000 | `GET /v1/sync/list` | Per-pull upper limit |
| Merkle tree depth | Max 12 | On-chain program validation | Max 4096 leaves per channel |
| Channel default tree depth | 4 | Config file | 16 leaves, ~350 bytes on-chain space |
| Hub list pagination | Default 100, hard cap 500 | Hub Registry API | Per-query upper limit |
| PostgreSQL connection pool | 10 | `max_connections(10)` | Hub Registry |
| Channel WS send buffer | 256 messages/connection | `mpsc::channel(256)` | Backpressure control |
| Hub metrics push interval | 60s | `publish_interval_secs` config | Hub → Registry |
| Multi-hop default max hops | 3 | `max_hops.unwrap_or(3)` | Route discovery default |

### 16.2 Merkle Tree Capacity

| tree_depth | Max Leaves | On-chain Account Space | Off-chain Storage (Vec\<UTXOLeaf\>) |
|:-----------|:-----------|:-------------|:---------------------------|
| 4 (default) | 16 | ~350 B | ~1-3 KB |
| 6 | 64 | ~800 B | ~5-11 KB |
| 8 (MCP default) | 256 | ~1.4 KB | ~18-35 KB |
| 10 | 1024 | ~4.3 KB | ~75-140 KB |
| 12 (max) | 4096 | ~16.6 KB | ~300-560 KB |

> Each channel state update rewrites the full `Vec<UTXOLeaf>`. At tree_depth=12, a single write is ~300-560 KB.

### 16.3 Data Growth Model

#### Growth by Event Type

| Event Type | Affected Services | Growth Per Event | Has Cleanup |
|:---------|:---------|:-----------|:-----------|
| Payment request (X402) | User MCP | ~300-600 B (PaymentRequest) | No |
| Payment order (QR) | Merchant MCP, Merchant App | ~400-600 B (PaymentOrder) | No |
| Audit log | User MCP, Merchant MCP | ~200-1000 B (AuditEntry) | **No, append-only** |
| Channel state update | Channel Service | Rewrites ~1-560 KB (depends on tree_depth) | Retained after channel close |
| Compliance audit | Channel Service | ~170-230 B (LeafUpdate) | **No, append-only** |
| DIDComm message | Router | ~1-10 KB (incl. encryption envelope) | Yes (TTL + capacity eviction) |
| HTLC record | Channel Service | ~176 B/entry, full Vec rewrite | cleanup() clears completed |
| Multi-hop payment | Channel Service | ~150 B/hop | **No** |
| On-chain operation fee | DID Registry | ~200-300 B | **No, append-only** |
| List change | User MCP (IPFS + sled) | ~200-400 B/merchant | IPFS full rebuild, local override |

#### Growth Formula Estimates

```
DIDComm Router disk ≈ Σ(per user min(message count, 1000) × 5 KB)
Channel Service disk ≈ Σ(per channel 2^depth × ~100 B) + Σ(per LeafUpdate ~200 B)
User MCP disk ≈ payment record count × 500 B + audit entry count × 600 B
Merchant MCP disk ≈ order count × 500 B + audit entry count × 300 B
```

### 16.4 sled Storage Details

#### Per-Service sled Tree Inventory

**DIDComm Router** (`./data`):

| Tree Name | Growth Dimension | Description |
|:-----|:---------|:-----|
| `msg:{recipient_did}` | Per-user independent tree, cap 1000 entries | Encrypted message queue |
| `keylist` | Per (session, recipient) pair | Routing forwarding map |
| `keylist_reverse` | Per recipient DID | Reverse routing map |
| `device_tokens` | Per user | FCM device tokens |
| `push_channels` | Per user | Push channel preference (fcm/websocket) |
| `agent_to_user` | Per Agent DID | Agent → user binding |
| `user_to_agents` | Per (user, Agent) pair | User → Agent index |

**Channel Service** (`{db_path}`):

| Key Pattern | Growth Dimension | Description |
|:---------|:---------|:-----|
| `channel:{id}:meta` | Per channel | Channel metadata (~220 B) |
| `channel:{id}:leaves` | Per channel | Full Merkle leaves (1-560 KB) |
| `channel:{id}:cosign` | Per channel | Co-signature (0 or 65 B) |
| `compliance:{id}` | Per channel | Compliance status (~200 B+) |
| `audit:{id}:{seq}` | Per channel per update | Audit entry (~200 B, **append-only**) |
| `htlc:{id}` | Per channel | HTLC record Vec |
| `multihop:{id}` | Per multi-hop payment | Multi-hop payment state |
| `hub:{hash}` | Per Hub | Hub registration (~228 B) |
| `hub_metrics:{hash}` | Per Hub | Hub metrics (~52 B, in-place update) |

**User MCP** (`./data`):

| Key Pattern | Growth Dimension | Description |
|:---------|:---------|:-----|
| `{payment_id}` (default tree) | Per payment | PaymentRequest (~300-600 B) |
| `__identity__` | Single entry | DID identity (~200 B) |
| `__audit_log__:{ts}:{uuid}` | Per event | Audit entry (**append-only**) |
| `session:{pubkey}` | Per Session Key | Temporary key + metadata (~200 B) |

**Merchant MCP** (`./data/merchant-mcp`):

| Tree Name | Growth Dimension | Description |
|:-----|:---------|:-----|
| `orders` | Per order | PaymentOrder (~400-600 B) |
| `merchant_audit` | Per event | Audit entry (**append-only**) |

**DID Registry** (`./did_registry_data`):

| Key Pattern | Growth Dimension | Description |
|:---------|:---------|:-----|
| `merchant:{hash}` | Per merchant | On-chain merchant record |
| `leaf_index:{hash}` | Per merchant | Leaf index (4 B) |
| `vc:{hash}` | Per VC | Verifiable Credential JSON |
| `fee:{op}:{ts}:{hash}` | Per on-chain operation | Fee record (**append-only**) |
| `revoked_vc:{hash}` | Per revocation | Revocation record |

#### sled Configuration

All services use `sled::open(path)` with default settings; no custom tuning:

| Parameter | Default | Description |
|:-----|:-------|:-----|
| Page cache | 256 MB | sled default |
| Background flush | 500ms | sled default |
| Explicit flush | Called after each critical write | 33 `.flush()` calls |

### 16.5 Data Lifecycle

#### With Cleanup Mechanism

| Data | Cleanup Method | Trigger Condition |
|:-----|:---------|:---------|
| DIDComm messages | FIFO eviction | `len > max_queued_messages` |
| DIDComm messages | TTL expiration | `age > max_message_age_seconds` |
| HTLC records | `cleanup()` | Fulfilled/refunded/expired |
| Compliance window payments | `retain()` | Outside slot window |
| Pending Session Key | `db.remove()` | After on-chain registration complete |
| Lists (IPFS sync) | `tree.clear()` + rebuild | On IPFS CID change |

#### Soft Expiry Without Deletion

| Data | Expiry Check Method | Actual Deletion |
|:-----|:------------|:---------|
| Whitelist/blacklist entries | Check `expires` field on read | Requires manual removal |
| Session Key (on-chain) | `is_expired()` check | Requires calling `close_session()` |
| HTLC (not cleaned up) | `check_expiry()` marks Expired | Requires calling `cleanup()` |

#### Append-Only, No Cleanup

- User MCP audit logs (`__audit_log__`)
- Merchant MCP audit logs (`merchant_audit`)
- Compliance audit entries (`audit:{channel_id}:{seq}`)
- DID Registry fee records (`fee:...`)
- Multi-hop payment records (`multihop:...`)
- DID Registry merchant records and VCs (logically should not be deleted)
- Hub registration and metrics records

### 16.6 Performance Characteristics and Bottlenecks

#### Write Amplification

Channel leaf data (`Vec<UTXOLeaf>`) is fully rewritten on each state update. For a channel with tree_depth=12, each payment generates ~300-560 KB of sled writes. This becomes a primary bottleneck under high tree depth + high-frequency payment scenarios.

Similarly, HTLC records (`Vec<HtlcRecord>`) also use a full rewrite pattern.

#### No Connection/Channel Count Limits

| Resource | Limit | Description |
|:-----|:-----|:-----|
| WebSocket connections | No upper limit | `DashMap` has no capacity limit |
| Per-user channels | No upper limit | Not limited at application layer |
| Per-Hub channels | No upper limit | Not limited at application layer |
| Concurrent HTLCs/channel | Implicit: 2^tree_depth - 1 | Constrained by leaf count |

#### Performance Benchmarks

The current codebase has no performance benchmarks or stress tests. No `criterion` or similar benchmarking framework.

### 16.7 Capacity Planning Recommendations

| Scenario | Daily Active Users | Channels | Daily Payments | Estimated Monthly Disk Growth |
|:-----|:---------|:-------|:---------|:---------------|
| Small-scale pilot | 100 | 50 | 500 | ~50 MB |
| Medium-scale | 1,000 | 500 | 5,000 | ~500 MB |
| Large-scale | 10,000 | 5,000 | 50,000 | ~5 GB |

> The above estimates are based on tree_depth=8 and median sizes for audit logs and message queues. Actual growth depends on tree_depth configuration and transaction frequency.

**Key Operations Recommendations**:

- Audit logs (MCP + compliance) grow append-only; implement periodic archiving or TTL-based cleanup
- Channels with tree_depth > 8 should be limited in number to avoid write amplification impacting latency
- DIDComm Router's `max_queued_messages` and `max_message_age_seconds` are key knobs for controlling disk growth
- In production, monitor sled data directory size and set alert thresholds
- All sled databases default to 256 MB cache; evaluate whether this is sufficient for large-scale deployments

---

## Appendix: Design System

Both Flutter Apps share the same Dark Glassmorphism design language:

---

## Document Change Log

| Version | Date | Changes |
|:-----|:-----|:---------|
| v0.5 | 2026-04-22 | Added performance and capacity planning (§16), optimized component dependency diagram (§2.4) |
| v0.4 | 2026-04-22 | Added error handling and retry strategy (§13), monitoring and observability (§14), deployment overview (§15) |
| v0.3 | 2026-04-22 | Added multi-hop payment flow (§4.4), complete DID Registry API paths (§9.4), Merchant MCP tool interface (§9.6) |
| v0.2 | 2026-04-21 | Initial complete version: architecture, components, data flow, protocols, storage, identity, security, API, push, compliance, on-chain program |

Both Flutter Apps share the same Dark Glassmorphism design language:

| Token | Value | Purpose |
|-------|-----|------|
| Background | `#0A0A14` | Page background |
| Surface | `#12121F` ~ `#22223A` | Cards, input fields, borders |
| Text Primary | `#F0F0F8` | Titles, amounts |
| Text Secondary | `#7A7A96` | Descriptions |
| Neon Cyan | `#00F5FF` | Primary accent, button gradient |
| Purple | `#8B5CF6` | Secondary accent |
| Success | `#00E676` | Confirmed, connected |
| Pending | `#FFB300` | Pending |
| Danger | `#FF5252` | Failed, closed, disconnected |

Fonts: Inter (body text) + JetBrains Mono (numeric values, DIDs, code).
