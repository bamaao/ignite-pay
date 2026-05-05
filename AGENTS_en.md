# AGENTS.md — Ignite Pay Project Structure

## Repository Overview

Ignite Pay is a Solana-based decentralized payment system comprising on-chain programs (Anchor), off-chain state channels, DID identity management, DIDComm secure communication, AI Agent payment orchestration, and a mobile SDK. The project uses a multi-crate repository structure with a total of 15 Rust crates + documentation.

---

## Dependency Layers

```
Layer 0 — Foundation Libraries
  ignite-pay-core              Shared types, DID identity, DIDComm, VC, audit logging
  ignite-pay-state-channel     Off-chain UTXO Merkle Tree state channel engine

Layer 1 — Solana Integration
  ignite-pay-solana            Solana RPC client, payment execution, ZK DID queries, session keys

Layer 2 — On-Chain Programs (no in-repo dependencies)
  ignite-pay-program           State channel on-chain program (10 instructions)
  ignite-pay-did-program       Merchant DID on-chain program (ZK Compression, 6 instructions)
  ignite-pay-session-program   Session key on-chain program (4 instructions)

Layer 3 — Services and Applications
  ignite-pay-channel-service   State channel HTTP service (User / Provider / Hub roles)
  didcomm-router               DIDComm message routing and mediator service
  did-registry                 DID on-chain registration and query service
  ignite-pay-mcp               AI Agent payment orchestration MCP server (includes state channel payments)
  ignite-pay-merchant-mcp      Merchant-side MCP server (QR payment code + state channel payment receipt)
  ignite-pay-skill             Python SDK (PyO3)
  ignite_pay_app               Flutter mobile app (Rust Bridge, includes QR code payment)

Layer 4 — Tests
  ignite-pay-litesvm-tests     State channel on-chain program tests (litesvm simulator)
  ignite-pay-mollusk-tests     State channel on-chain program tests (mollusk simulator)
```

---

## Crate Descriptions

### 1. ignite-pay-core

**Location**: `ignite-pay-core/`
**Type**: Library (lib)
**Purpose**: Foundation library shared across the entire project, providing DID identity, DIDComm encrypted communication, Verifiable Credentials (VC), whitelist/blacklist risk control, and E2EE audit logging.

**Key Modules**:

| Module | Description |
|:-------|:------------|
| `identity` | `did:ignite` decentralized identity generation (Ed25519 + X25519), DID Document construction, persistence |
| `didcomm` | DIDComm message creation, JWE encryption wrapping/unwrapping, DIDComm protocol implementation (including QR payment request/confirmation messages) |
| `vc` | Verifiable Credential issuance, verification, IPFS resolution |
| `ipfs` | IPFS client abstraction (trait + Kubo implementation + Mock) |
| `list_store` | sled-based whitelist/blacklist persistence and risk control decisions |
| `types` | Shared types: `MerchantListEntry`, `RiskControlDecision`, `VerifiableCredential` |
| `audit_merkle` | Merkle tree audit logging (E2EE logs) |
| `log_crypto` | E2EE log encryption (HKDF + AES-GCM + zstd compression) |
| `log_chunk` / `log_sync` | Log chunking and synchronization |
| `solana_did` | Solana DID on-chain bridging (requires `solana` feature) |

**Optional features**: `kubo` (IPFS Kubo client), `solana` (on-chain DID bridging)

---

### 2. ignite-pay-state-channel

**Location**: `ignite-pay-state-channel/`
**Type**: Library (lib)
**Purpose**: Off-chain UTXO + Merkle Tree state channel engine supporting streaming payments, HTLC, multi-hop routing, Hub network, and compliance management.

**Key Modules**:

| Module | Description |
|:-------|:------------|
| `channel` | `ChannelManager` — Full channel lifecycle management (open, fund, pay, settle) |
| `merkle` | Merkle tree construction and verification |
| `signing` | Ed25519 signing utilities (leaf signing, state signing, key generation) |
| `types` | Channel data structures (UTXO leaves, channel metadata, channel state) |
| `pipeline` | Payment pipeline processing |
| `htlc` | `HtlcManager` — HTLC preimage management, lifecycle tracking |
| `hub` | `HubManager` — Hub registration, metrics management |
| `routing` | `RouteService` — DFS route discovery, scoring, selection |
| `multihop` | `MultiHopManager` — Multi-hop HTLC payment coordination, decrementing timelocks |
| `compliance` | `ComplianceManager` — Spending limits, sliding windows, compliance flags, audit trails |

---

### 3. ignite-pay-solana

**Location**: `ignite-pay-solana/`
**Type**: Library (lib)
**Purpose**: Solana blockchain integration layer, providing RPC client wrappers, on-chain payment execution, ZK Compression DID queries, session key management, and state channel on-chain instruction builders.

**Key Modules**:

| Module | Description |
|:-------|:------------|
| `channel` | 10 on-chain instruction builders (open, fund, settle, challenge, claim, HTLC, etc.) |
| `payment` | `IgnitePayClient` — On-chain payment execution (supports Sponsored and SelfFunded modes) |
| `compression` | ZK Compression DID queries (Light Protocol `light-sdk`) |
| `session` | Session key management (`SessionKeypair`, `SessionTokenData`) |
| `session_program` | Session program client integration |
| `types` | `PayMode`, `SessionTokenData`, `SplPaymentParams`, and other types |

---

### 4. ignite-pay-program

**Location**: `ignite-pay-program/`
**Type**: On-chain program (cdylib + lib) — Anchor framework
**Program ID**: `DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe`
**Purpose**: Solana on-chain program for state channels, managing the full lifecycle of channel accounts and fund security.

**On-Chain Instructions (10)**:

| Instruction | Description |
|:------------|:------------|
| `open_channel` | Open a channel, verify Ed25519 signatures, initialize Merkle tree |
| `fund_channel` | Provider funding (SPL Token transfer) |
| `cooperative_settle` | Cooperative settlement (both party signatures) |
| `trigger_challenge` | Initiate a dispute challenge |
| `submit_counter_state` | Submit counter state |
| `settle_after_timeout` | Settle after timeout |
| `claim` | Claim a leaf (Merkle Proof + signature) |
| `verify_htlc` | Verify HTLC preimage and claim |
| `htlc_refund` | Expired HTLC refund |
| `finalize_settlement` | Final settlement, proportionally distribute unclaimed balances |

**Key Data Structures**: `ChannelAccount` (channel account), `ChannelStatus` (status enum)

---

### 5. ignite-pay-did-program

**Location**: `ignite-pay-did-program/`
**Type**: On-chain program (cdylib + lib) — Anchor framework
**Purpose**: Merchant DID on-chain identity management, using ZK Compression (Light Protocol) for low-cost on-chain DID registration and verification.

**On-Chain Instructions (6)**:

| Instruction | Description |
|:------------|:------------|
| `init_platform` | One-time platform public key initialization |
| `initialize_did` | Create a compressed merchant DID (platform signature verification + subject binding) |
| `update_did_with_vc` | Bind/update VC Hash (controller only) |
| `set_recovery_key` | Set/replace recovery key |
| `recover_controller` | Recover controller via recovery key |
| `revoke_vc` | Revoke VC (platform authority only) |

**Key Data Structures**: `MerchantCompressedDid`, `PlatformConfig`, `RevokedVc`

---

### 6. ignite-pay-session-program

**Location**: `ignite-pay-session-program/`
**Type**: On-chain program (cdylib + lib) — Anchor framework
**Program ID**: `6EFvVTh7rEBpHH2JGryjKQmBLRtbYtSEerGNfkHqKiei`
**Purpose**: On-chain session key management, allowing temporary keys to execute payments within a limited scope without exposing the master key.

**On-Chain Instructions (4)**:

| Instruction | Description |
|:------------|:------------|
| `register_session_key` | Register a temporary session key (scope, limits, expiry time) |
| `execute_payment` | Execute SOL transfer using session key (checks scope, limits, expiry) |
| `revoke_session` | Revoke session key (owner only) |
| `close_session` | Close and recover rent (must be revoked or expired) |

---

### 7. ignite-pay-channel-service

**Location**: `ignite-pay-channel-service/`
**Type**: Three binaries (bin) — Axum 0.8 HTTP + WebSocket service
**Purpose**: State channel REST + WebSocket server, providing independently deployable binaries for the User, Provider, and Hub roles.

**Binary Targets**:

| Binary | Role | Default Port | Description |
|:-------|:-----|:-------------|:------------|
| `channel-user` | User | 3001 | User side: open channels, initiate payments, HTLC management, route discovery |
| `channel-provider` | Provider | 3002 | Merchant side: receive payments, co-sign confirmations, settlement claims |
| `channel-hub` | Hub | 3003 | Hub side: inherits all Provider functionality + route relay + multi-hop payments |

**Architecture**: Hub inherits all Provider endpoints; Provider is a subset of Hub. User endpoints are entirely different from the other two roles.

**Key Modules**:

| Module | Description |
|:-------|:------------|
| `config` | TOML configuration loading (`Config`, `Role` enum) |
| `state` | `AppState` shared state (Arc-wrapped, containing sled DB, ChannelManager, HubManager, etc.) |
| `server/router` | Role-based Axum route builder |
| `ws` | WebSocket protocol definitions and authenticated session management |
| `handlers` | HTTP request handlers (channel, payment, settlement, htlc, routing, multihop, compliance) |
| `storage` | sled storage layer (channel index, node registry) |

---

### 8. didcomm-router

**Location**: `didcomm-router/`
**Type**: Binary (bin) — Axum HTTP + WebSocket service
**Purpose**: DIDComm message routing and mediator service, responsible for securely forwarding encrypted messages between DID peers, supporting the mediator protocol (mediate-request/grant, keylist-update).

**Key Modules**:

| Module | Description |
|:-------|:------------|
| `server` | Axum route construction (HTTP + WebSocket) |
| `session` | WebSocket session management (authentication, message routing) |
| `protocols` | DIDComm protocol implementation (mediate-request, keylist-update, peer-introduction, connection-request) |
| `transport` | WebSocket transport layer |
| `notification` | Push notifications (FCM integration) |
| `did` | DID Document processing |
| `storage` | Persistent storage (sled) |

---

### 9. did-registry

**Location**: `did-registry/`
**Type**: Binary (bin) — Axum HTTP service
**Purpose**: DID on-chain registration and query service, providing REST APIs to register merchant DIDs on the Solana blockchain (with ZK Compression support) and query on-chain DID information.

**Key Modules**:

| Module | Description |
|:-------|:------------|
| `handlers` | HTTP request handlers (registration, query) |
| `did` | DID registration and query logic |
| `server` | Axum route construction |
| `storage` | Persistent storage (sled) |

---

### 10. ignite-pay-mcp

**Location**: `ignite-pay-mcp/`
**Type**: Binary (bin) — MCP server
**Purpose**: AI Agent payment orchestration server, exposing payment tools via the Model Context Protocol (MCP), processing x402 HTTP payment challenges, integrating DIDComm encrypted authorization and on-chain Solana payments. V3.0 adds state channel payment capabilities, automatically falling back to state channel payments when no active session key is available.

**MCP Tools**:

| Tool | Description |
|:-----|:------------|
| `process_x402_challenge` | Complete x402 payment flow (parse challenge -> verify VC -> on-chain DID check -> risk control -> phone authentication -> execute payment), supporting both session key and state channel payment modes |
| `check_authorization` | Check payment status |
| `get_payment_history` | Query payment history |
| `get_identity` | View DID and connection status |
| `generate_pairing_invitation` | Generate phone pairing QR code |
| `create_session` / `get_session_status` / `close_session` | Session key management |
| `execute_spl_payment` | Execute SPL Token transfer via session key |
| `add_merchant` / `update_merchant` / `verify_merchant` | On-chain ZK DID management |
| `open_channel` | Establish a state channel with a Hub (User role) |
| `channel_pay` | Initiate a payment through a state channel |
| `get_channel_status` | Query channel status (balance, sequence number, leaf count) |
| `close_channel` | Cooperatively close a state channel |
| `settle_channel` | Initiate on-chain settlement |

**Key Modules**:

| Module | Description |
|:-------|:------------|
| `channel` | State channel User-side client (`ChannelClient`), communicating with Hub HTTP API |
| `payment` | Payment storage, pending authorization storage, payment types |
| `mediator` | DIDComm Mediator WebSocket connection (pairing/invitation) |
| `audit` | Payment and list event audit storage |
| `tools` | MCP tool input type definitions |

---

### 11. ignite-pay-skill (ignite_pay_rs)

**Location**: `ignite-pay-skill/`
**Type**: Python extension module (cdylib) — PyO3
**Python Package Name**: `ignite_pay_rs`
**Purpose**: Python bindings for the Web3 Agent Payment SDK, exposing core DID identity, DIDComm communication, risk control, and payment functionality to the Python ecosystem.

**Python Class `IgnitePayCore` Methods**:

| Method | Description |
|:-------|:------------|
| `new()` | Generate a new DID identity |
| `init_identity(db_path)` | Load/generate persistent identity from sled |
| `init_list_store(db_path)` | Initialize whitelist/blacklist storage |
| `start_listener(ws_url)` | Start a background WebSocket listener to connect to mediator |
| `check_allowance(merchant_did, amount)` | Query merchant whitelist/blacklist |
| `risk_check(merchant_did, amount)` | Risk control decision |
| `check_and_pay(merchant_did, amount)` | Core payment flow (includes phone authorization) |
| `add_to_whitelist` / `remove_from_whitelist` | Whitelist management |
| `add_to_blacklist` / `remove_from_blacklist` | Blacklist management |

---

### 12. ignite_pay_app (rust_lib_ignite_pay_app)

**Location**: `ignite_pay_app/rust/`
**Type**: Flutter Rust Bridge library (cdylib + staticlib)
**Purpose**: Rust native layer for the Ignite Pay mobile Flutter app, providing DID identity, DIDComm communication, and other native functionality via Flutter Rust Bridge. Supports QR code payment: parse merchant QR code -> confirm payment -> complete payment via state channel.

**Key Modules**:

| Module | Description |
|:-------|:------------|
| `api/channel` | State channel bridge functions (parse QR, open channel, pay, close, settle) |
| `api/channel_store` | Channel state sled persistence (`ChannelStore`) |
| `api` | Flutter-callable API functions |
| `frb_generated` | Flutter Rust Bridge auto-generated bindings |

**Flutter Layer Key Files**:

| File | Description |
|:-----|:------------|
| `lib/services/channel_service.dart` | Dart channel service layer (`ChannelService`) |
| `lib/qr_payment_screen.dart` | QR code payment confirmation UI (dark glassmorphism style) |

---

### 13. ignite-pay-merchant-mcp

**Location**: `ignite-pay-merchant-mcp/`
**Type**: Binary (bin) — MCP server
**Purpose**: Merchant-side AI Agent MCP server. Generates payment QR codes, receives state channel payments, manages orders and payment records. The merchant acts as the Provider role in the state channel.

**MCP Tools**:

| Tool | Description |
|:-----|:------------|
| `generate_payment_qr` | Generate a payment QR code (`ignite://pay?d=<base64url>` format) |
| `check_payment` | Query payment status by order_id |
| `get_payment_history` | Payment receipt history |
| `get_channel_status` | Channel status (balance, sequence number, Provider balance) |
| `open_channel_with_hub` | Prompt merchant Provider pubkey for user to open a channel |
| `close_channel` | Cooperatively close channel |
| `settle_channel` | On-chain settlement (claim + finalize) |
| `get_identity` | Merchant DID, Hub connection status |

**Key Modules**:

| Module | Description |
|:-------|:------------|
| `channel` | `MerchantChannelClient` — Provider role state channel client (receive payments, co-sign, settle) |
| `payment` | `PaymentOrderStore` — Order sled persistence (create, confirm, query, list) |
| `qr` | QR code generation and parsing (`PaymentQrData` struct, `ignite://pay` protocol format) |
| `mediator` | `MerchantMediator` — DIDComm Mediator connection (send payment confirmation messages) |
| `audit` | `AuditLogStore` — Merchant operation audit log |
| `config` | TOML configuration loading (merchant, mediator, storage, solana, hub) |
| `tools` | MCP tool input type definitions |

**QR Code Format**: `ignite://pay?d=<base64url(JSON)>`, where JSON contains `type: "ignite-pay-request"`, `merchant_did`, `amount`, `description`, `order_id`, `hub_endpoint`, `timestamp`.

---

### 14. ignite-pay-litesvm-tests

**Location**: `tests/svm-litesvm/`
**Type**: Test library (lib)
**Purpose**: Integration tests for the `ignite-pay-program` on-chain program using the litesvm SVM simulator.

**Covered Test Scenarios**: open_channel signature verification, trigger_challenge, cooperative_settle, submit_counter_state, settle_after_timeout, challenge-not-yet-expired rejection, settle status checks.

---

### 15. ignite-pay-mollusk-tests

**Location**: `tests/svm-mollusk/`
**Type**: Test library (lib)
**Purpose**: Integration tests for the `ignite-pay-program` on-chain program using the Mollusk SVM simulator. Covers the same test scenarios as the litesvm tests but uses a different SVM simulator backend.

---

## Documentation

| Directory | Description |
|:----------|:------------|
| `docs/` | Design documents: state channel specification, DID identity scheme, DIDComm communication protocol, session keys, audit logging, etc. |
| `docs/deploy/` | Deployment documents: ZK DID deployment guide, merchant DID on-chain walkthrough, state channel implementation details |
| `docs/deploy/state-channel/` | State channel deployment configuration: User, Hub, and Merchant service deployment documentation |
| `docs/deploy/state-channel/scenarios/` | 12 business scenario implementation documents: channel opening, off-chain payments, batch pipeline, HTLC, cooperative close, dispute resolution, HTLC settlement, Hub routing, multi-hop payments, auto-close, compliance audit, WebSocket real-time communication |

---

## Technology Stack

| Category | Technology |
|:---------|:-----------|
| Blockchain | Solana (Anchor framework) |
| Zero-Knowledge Compression | Light Protocol (ZK Compression) |
| Encrypted Communication | DIDComm v2 (JWE), Ed25519, X25519 |
| Identity | `did:ignite` (W3C DID compatible), Verifiable Credentials |
| Backend | Rust, Axum 0.8, sled, tokio |
| On-Chain Testing | litesvm, mollusk |
| Python Bindings | PyO3 |
| Mobile | Flutter + Flutter Rust Bridge |
| AI Integration | MCP (Model Context Protocol), x402 payment protocol, state channel QR code payment |
| Message Mediation | DIDComm Router (WebSocket) |
