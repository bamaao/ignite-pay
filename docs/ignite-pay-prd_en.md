# Ignite Pay — Product Document

## 1. System Overview

Ignite Pay is a Solana-based decentralized payment system consisting of three core components:

```
┌─────────────┐    DIDComm    ┌──────────┐    DIDComm    ┌──────────────┐
│  User App   │◄────────────►│ Mediator │◄────────────►│ Merchant App │
│ (Sentinel)  │   Encrypted  │  Relay   │   Encrypted  │  (Merchant)  │
│             │   Messages   │  Service │   Messages   │              │
└──────┬──────┘               └──────────┘               └──────┬───────┘
       │                                                        │
       │  MB Voucher Signing          MB Voucher Collection      │
       ▼                                                        ▼
┌─────────────────────────────────────────────────────────────────────┐
│                  MagicBlock Payment Channel (On-chain)              │
│  init_global · deposit · create_channel · settle_batch · release    │
│  optimistic_settle · dispute · resolve_dispute · withdraw           │
│  off-chain: sign_voucher · receive_voucher · merkle proof           │
└─────────────────────────────────────────────────────────────────────┘
       │
       ▼
┌─────────────────────────────────┐
│  Solana Blockchain              │
│  MagicBlock Payment Channel Contract │
│  Session Key Contract           │
└─────────────────────────────────┘
```

**Core Concept:** Users control their own DID identity and funds through a mobile App, providing real-time authorization for payments initiated by AI agents; merchants generate payment QR codes and receive instant payment confirmations through a mobile App. Both ends communicate via DIDComm end-to-end encrypted messaging, relayed through a Mediator. Payments are implemented through MagicBlock payment channels: on-chain locked funds + off-chain signed Vouchers + Merkle Sum Tree batch settlement.

---

## 2. User App — Sentinel Dashboard

### 2.1 Product Positioning

The user's "Payment Guardian" App. Manages DID identity, receives and approves payment requests initiated by AI agents, manages Session Keys, and maintains whitelist/blacklist policies.

### 2.2 Tech Stack

| Layer | Technology |
|------|------|
| UI Framework | Flutter (Dart) |
| Crypto/Storage | Rust (via flutter_rust_bridge) |
| Local Database | sled (identity, Session Keys, policies, channels) + SQLite (audit logs) |
| Message Transport | DIDComm v2 (JWE authcrypt encryption) |
| Push Channel | FCM (international) / WebSocket persistent connection (domestic/China) |
| Blockchain | Solana (MagicBlock payment channel contract + Session Key contract) |

### 2.3 Core Identity

Users possess a `did:ignite:z<multicodec_base58>` decentralized identity, containing:
- Ed25519 signing key
- X25519 key agreement key (for JWE encryption/decryption)
- W3C DID Document

The identity is stored in the sled local database and generated on first launch.

### 2.4 Feature Modules

#### 2.4.1 Onboarding Flow (OnboardingScreen)

Three-step onboarding: Welcome page -> Generate DID identity -> Configure Mediator connection (skippable).

#### 2.4.2 Dashboard

- DID identity card (displaying DID, connection status, pending message count)
- Notification bell: unread message count badge, tap to enter notification center
- Quick access: Vault (key store), Policies (policy management), Channels (channel topology)
- "Scan MCP QR Code" button: scan merchant QR code to establish pairing (supports PaymentQrData and didcomm:// pairing)
- "Create Channel" button: create a channel based on MagicBlock payment channel, specifying merchant public key and spending limit
- Trust limit dashboard: today's spending / limit
- Recent activity list: real-time data, fetched from `DidcommService.messages`
- Payment authorization banner: pops up when `payment-auth-request` is received

#### 2.4.3 Payment Authorization (ChallengeScreen — X402 Protocol)

Core interaction interface. Full-screen authorization page pops up after receiving an MCP payment request:

1. **Displayed information**: Merchant DID, amount (SOL), payment reason
2. **Policy configuration**: Daily limit, daily transaction count limit, per-transaction limit, validity period
3. **List operations**: One-time only / Add to whitelist / Add to blacklist / Remove from whitelist / Remove from blacklist
4. **Signing method selection**: Built-in key / Phantom deep link / Solflare deep link / MWA (Android)
5. **Actions**: Approve (create Session Key -> on-chain registration -> reply to MCP) or Decline & Block

#### 2.4.4 Policy Management (PolicyScreen)

Configure spending rules per merchant:
- Auto-pay toggle
- Per-transaction limit (SOL/USD switch)
- Weekly spending progress bar
- Validity countdown

#### 2.4.5 Key Vault (VaultScreen)

- DID identity display (with animated gradient background)
- 12-word mnemonic (show/hide toggle)
- Mediator endpoint configuration
- Audit log viewer
- "Erase Key Material" dangerous operation

#### 2.4.6 Session Key Management (SessionKeysScreen)

Full lifecycle of on-chain Session Keys:
- Register new Key (5 SOL / 24h default parameters)
- View active/expired Key list
- Revoke (on-chain) / Delete (local)

#### 2.4.7 QR Scanner (QrScannerScreen)

Full-screen camera scanning for MCP pairing QR codes in `didcomm://?_oob=<base64>` format. After scanning, parses the OOB invitation, connects to Mediator, and sends `connection-request`.

#### 2.4.8 Channel Payment (QrPaymentScreen)

Confirmation page after scanning merchant payment QR code `ignite://pay?d=<base64>`:
- Display merchant information, amount, description
- Confirm -> Sign Voucher (Ed25519 signature of `SHA256(channel_id || seq || amount)`) -> Send to merchant
- Display payment result (sequence, voucher signature)

#### 2.4.9 Message List (MessagesScreen)

DIDComm message inbox:
- Filter tabs: All / Payment / List Sync / Connection
- Tap `payment-auth-request` message to trigger authorization flow
- View message details (including raw body)

#### 2.4.10 Connection Management (ConnectionScreen)

- Mediator connection status (WS/FCM channel indicator)
- Paired MCP agent list
- Add new MCP (opens QR scanner)

#### 2.4.11 Settings (SettingsScreen)

- Solana network: Devnet / Mainnet switch, RPC URL, DAS Endpoint
- SPL account compression parameters: Tree Address, Tree Authority
- Program IDs: MagicBlock payment channel, DID, Session Key (read-only)
- Payment mode: Self-pay / Sponsored
- Storage management: Clear cache

#### 2.4.12 Notification Center (NotificationCenterScreen)

System notifications and connection update message list:
- Message list: filter non-payment-auth-request type messages from `DidcommService.messages`
- Read/unread status: store read IDs via SharedPreferences
- Mark all as read
- Notification detail popup: display message type, CID, tags, description, RAW BODY
- Access from Dashboard notification bell icon (with unread count badge)

#### 2.4.13 Channel Topology (ChannelTopologyScreen)

MagicBlock payment channel network visualization and management:
- Total balance card: display global Vault balance (SOL) + total deposited + total allocated
- Local node card: display user DID + MB Buyer Pubkey + connection status pulse animation
- Channel card list: merchant public key, spending cap, settled amount, current batch nonce, challenge period / dispute period
- Actions: adjust spending cap (`update_spending_cap`), dispute (`dispute`), resolve dispute (`resolve_dispute`)
- Pull to refresh
- Empty state / error state / loading state

#### 2.4.14 Transaction History (TransactionHistoryScreen)

Transaction record browser:
- Filter tabs: All / Payment / List Sync
- Transaction card list: Merchant DID, amount (SOL), status badge (Pending/Processed)
- Transaction detail popup: type, Payment ID, merchant, amount, description, RAW BODY
- Pull to refresh (reconnect to Mediator to fetch latest messages)
- Data source: `DidcommService.messages` filtered by type

#### 2.4.15 Profile (ProfileScreen)

User identity and account overview:
- DID avatar (first two characters)
- DID display (copyable)
- Edit display name (persisted via SharedPreferences)
- Network information: Devnet / Mainnet display switch
- Device status: connection status indicator + Session Key activation status badge
- Statistics cards: Channel count / Balance (SOL) / Merchant count
- Export DID Document (copy to clipboard)

#### 2.4.16 MB Payment Channel Management (MbChannelScreen)

MagicBlock payment channel configuration:
- Configure MB RPC URL and Program ID
- Global state initialization (`init_global`): create GlobalState + GlobalVault PDA
- Deposit (`deposit`): transfer SOL to GlobalVault
- Create channel: specify merchant public key, spending cap, challenge period, dispute period
- Withdraw unallocated funds (`withdraw`)
- Access from Dashboard "Create Channel" button

### 2.5 Core Services

| Service | Responsibility |
|------|------|
| `DidcommService` | DID identity management, Mediator connection, message send/receive, authentication, push orchestration |
| `SessionKeyService` | Session Key creation/registration/revocation/query, supports built-in keys and external wallets |
| `ChannelService` | MagicBlock payment channel operations (initialize global state, deposit, create channel, sign Voucher, dispute/resolve dispute, withdraw) |
| `FcmService` | Firebase push notifications |
| `MediatorApi` | Mediator REST API HTTP client |
| `WalletDeepLinkService` | Phantom / Solflare wallet deep link builder |
| `WalletMwaService` | Mobile Wallet Adapter (Android, stub implementation) |

### 2.6 Rust Bridge Function List

| Function | Purpose |
|------|------|
| `initialize_identity` | Generate/load DID identity |
| `connect_mediator` | Connect to Mediator WebSocket |
| `disconnect_mediator` | Disconnect |
| `authenticate_with_mediator` | Challenge-response authentication to obtain JWT |
| `pull_messages` | Pull JWE messages from Mediator |
| `decrypt_message` | Decrypt JWE to extract payment fields |
| `send_auth_response` | Send payment authorization response (with Session Key) |
| `register_device_token` | Register FCM device token |
| `parse_oob_invitation` | Parse OOB invitation QR code |
| `send_connection_request` | Send connection request to MCP |
| `create_session_key_for_payment` | Create temporary Session Key |
| `build_unsigned_register_tx` | Build unsigned registration transaction (for external wallet) |
| `complete_register_with_signature` | Complete registration |
| `revoke_session_key_onchain` | On-chain Session Key revocation |
| `save_merchant_policy` / `load_merchant_policy` | Merchant policy persistence |
| `parse_payment_qr` | Parse payment QR code |
| `mb_init_global` | Initialize global state (create GlobalState + GlobalVault PDA) |
| `mb_deposit` | Deposit SOL to GlobalVault |
| `mb_create_channel` | Create payment channel (specify merchant, spending cap, challenge period, dispute period) |
| `mb_update_spending_cap` | Adjust channel spending cap |
| `mb_get_channel` / `mb_get_global_state` | Query channel/global state |
| `mb_sign_voucher` | Sign Voucher (Ed25519 signature of `SHA256(channel_id \|\| seq \|\| amount)`) |
| `mb_sign_settlement` | Sign settlement message (verify Merkle Root then sign) |
| `mb_dispute` / `mb_resolve_dispute` | Dispute/resolve dispute (submit Merkle Proof fraud proof) |
| `mb_withdraw` | Withdraw unallocated funds |

### 2.7 Complete Payment Authorization Flow

```
1. MCP agent initiates payment-auth-request
2. Mediator pushes to user App (FCM or WS)
3. App pulls JWE -> decrypt -> display authorization request
4. User reviews on ChallengeScreen:
   a. Confirm amount, merchant
   b. Optional: adjust policy parameters
   c. Optional: add to whitelist/blacklist
   d. Select signing method
5. App creates Session Key -> on-chain registration transaction -> confirm
6. Send payment-auth-response (with Session Key data) back to MCP
7. MCP uses Session Key to execute payment
```

---

## 3. Merchant App — Ignite Merchant

### 3.1 Product Positioning

The merchant's payment collection tool. Generate payment QR codes, receive instant payment confirmations, manage MagicBlock payment channels, and provide voice payment notifications.

### 3.2 Tech Stack

| Layer | Technology |
|------|------|
| UI Framework | Flutter (Dart) |
| Crypto/Storage | Rust (via flutter_rust_bridge) |
| Local Database | sled (orders, keys, channels, DIDComm identity) |
| Message Transport | DIDComm v2 (JWE authcrypt encryption) |
| Push Channel | FCM (international) / WebSocket persistent connection (domestic/China) |
| Voice Notification | flutter_tts (Chinese/English bilingual) |

### 3.3 Dual DID Architecture

The Merchant App manages two independent identities:

| Identity | DID Format | Purpose | Storage Location |
|------|----------|------|----------|
| MagicBlock Channel DID | `did:ignite:<raw_base58>` | QR code generation, Voucher collection, on-chain settlement signing | sled `mb_keys` tree |
| DIDComm Communication DID | `did:ignite:z<multicodec_base58>` | JWE encryption/decryption, Mediator message send/receive | sled `didcomm_identity` tree |

The two key systems are completely independent and do not interfere with each other.

### 3.4 Feature Modules

#### 3.4.1 Onboarding Flow (OnboardingScreen)

1. Enter MagicBlock RPC URL and Program ID
2. Enter Mediator WebSocket URL (optional)
3. Generate merchant identity (Ed25519 key pair -> MB Merchant Keypair)
4. Initialize push service (connect to Mediator)

#### 3.4.2 Dashboard (DashboardScreen)

- "Ignite Merchant" header + online status indicator ("Online")
- Notification bell: unread order count badge, tap to enter notification center
- Today's summary card: total received amount (USDC) + transaction count (confirmed only)
- Quick actions: Generate payment QR code / Channel management / MB configuration
- Recent order list: tap to enter order details

#### 3.4.3 Generate Payment QR Code (QrGenerateScreen)

Core payment collection interface:

1. Enter amount (USDC) + optional description
2. Generate QR code: format `ignite://pay?d=<base64url(JSON)>`
3. Display QR code, enter waiting-for-confirmation state
4. **Dual-channel waiting**:
   - Push confirmation (primary channel): listen to `MerchantPushService.confirmations` stream
   - Polling fallback (5-second interval): call `refreshOrders()` to check order status
5. After confirmation: green checkmark + haptic feedback + voice notification

**QR Code Payload Structure (PaymentQrData)**:
```json
{
  "type": "ignite-pay-request",
  "version": 1,
  "merchant_did": "did:ignite:...",
  "merchant_pubkey": "MB merchant Ed25519 base58 pubkey",
  "amount": 1000000000,
  "description": "Coffee",
  "order_id": "uuid-v4",
  "timestamp": 1713700000
}
```

#### 3.4.4 Payment List (PaymentListScreen)

- Filter: All / Pending confirmation / Confirmed
- Pull to refresh
- Order card list (amount, status badge, description, time)

#### 3.4.5 Order Details (PaymentDetailScreen)

- Amount display (large font USDC)
- Status badge: confirmed=green / pending=amber / failed=red / expired=gray
- Order information: Order number (copyable), description, creation time, confirmation time
- Channel information (confirmed only): Channel ID (copyable), Voucher Seq, Buyer Signature

#### 3.4.6 Channel Management (ChannelScreen)

- Summary: Total channels + cumulative received (USDC)
- Channel card list (Buyer Pubkey, Spending Cap, Settled Amount, Nonce)
- Pull to refresh

#### 3.4.7 Channel Details (ChannelDetailScreen)

- Channel information display: Buyer Pubkey, Spending Cap, Settled Amount, Nonce, Challenge Period, Dispute Period
- Action buttons:
  - **Batch settlement**: `settle_batch` (build Merkle Sum Tree + dual signatures) or `optimistic_settle` (merchant signature only)
  - **Release settlement**: `release_settlement` (after challenge period)
  - **Force release**: `force_release` (after dispute period)

#### 3.4.8 Settings (SettingsScreen)

- Merchant identity: MB Merchant Pubkey (copyable), MB Program ID (read-only)
- Connection configuration: MB RPC URL (editable), Mediator WS (status indicator)
- Push service: DIDComm DID (copyable), Mediator connection status, push channel type
- Voice notification: toggle, language switch (Chinese/English), volume slider, test button
- About: Version 1.0.0

#### 3.4.9 Notification Center (NotificationCenterScreen)

Merchant notification list:
- Convert from `MerchantService.orders` to notifications (payment received / pending confirmation)
- Read/unread status management (SharedPreferences `merchant_read_notification_ids`)
- Mark all as read
- Notification detail popup: order number, amount, description, status, channel
- Access from Dashboard notification bell icon (with unread count badge)

#### 3.4.10 Profile (ProfileScreen)

Merchant identity and account overview:
- DID avatar (first two characters or "M")
- DID display (copyable) + DID document export
- Edit merchant name (persisted via SharedPreferences)
- Network information: Devnet / Mainnet display
- Connection status: push service connection indicator + MB RPC URL display
- Statistics cards: Channel count / Balance (SOL) / Confirmed order count
- Access from Dashboard profile entry

#### 3.4.11 MB Configuration (MbConfigScreen)

MagicBlock payment channel configuration:
- MB RPC URL and Program ID configuration
- View merchant MB Keypair
- Access from Dashboard quick actions

### 3.5 Core Services

| Service | Responsibility |
|------|------|
| `MerchantService` | Merchant identity, order management, QR generation, configuration persistence |
| `MerchantPushService` | Dual-channel push orchestration (WS/FCM), message decryption, order confirmation |
| `ChannelService` | MagicBlock payment channel: Voucher collection, batch settlement, optimistic settlement, release |
| `VoiceService` | Payment received voice notification (Chinese/English bilingual) |
| `FcmService` | Firebase push notifications |
| `MediatorApi` | Mediator REST API HTTP client |

### 3.6 Rust Bridge Function List

**merchant.rs — MagicBlock Payment Channel & Orders:**

| Function | Purpose |
|------|------|
| `initialize_merchant` | Generate/load merchant MB key pair |
| `generate_merchant_keypair` | Generate Ed25519 key pair |
| `get_merchant_pubkey` | Get base58 public key |
| `generate_payment_qr` | Create order + generate QR string |
| `list_orders` / `get_order` / `get_pending_orders` | Order queries |
| `confirm_order` | Order status pending -> confirmed |
| `mb_get_channel` | Query channel status (Buyer/merchant channel PDA) |
| `mb_receive_voucher` | Verify buyer Voucher signature and store |
| `mb_settle_batch` | Build Merkle Sum Tree, dual-signature batch settlement |
| `mb_optimistic_settle` | Merchant-only signature optimistic settlement |
| `mb_get_settlement` | Query settlement Escrow account |
| `mb_release_settlement` | Release settlement funds to merchant |
| `mb_force_release` | Force release after dispute period |

**merchant_didcomm.rs — DIDComm Communication:**

| Function | Purpose |
|------|------|
| `initialize_merchant_comm` | Generate/load DIDComm identity (independent from MB channel Keypair) |
| `connect_mediator` | Connect to Mediator |
| `disconnect_mediator` | Disconnect |
| `authenticate_with_mediator` | Challenge-response authentication to obtain JWT |
| `pull_messages` | Pull JWE messages from Mediator |
| `decrypt_message` | Decrypt JWE to extract payment confirmation fields |
| `register_device_token` | Register FCM device token |

### 3.7 Complete Payment Collection Flow

```
1. Merchant enters amount + description on QR Generate page
2. App calls Rust generate_payment_qr()
   -> Create UUID order (status=pending)
   -> Return ignite://pay?d=... QR code string (containing merchant MB Pubkey)
3. User App scans code -> parse PaymentQrData
4. User confirms payment -> User App signs Voucher (Ed25519 signature of SHA256(channel_id || seq || amount))
5. Voucher sent to merchant via DIDComm -> Mediator pushes to Merchant App
6. Merchant App calls mb_receive_voucher() to verify buyer signature and store
7. Merchant App confirm_order() updates order status to confirmed
8. Triggers:
   - QR page green checkmark
   - Haptic feedback
   - Voice notification ("Received payment X.XX USDC")
   - Dashboard today's summary refresh
9. Subsequent settlement flow (merchant-initiated):
   a. mb_settle_batch(): build Merkle Sum Tree, merchant signs, submit on-chain settlement
   b. or mb_optimistic_settle(): merchant-only signature (use settle_batch when buyer cooperation is needed for settlement signature)
   c. After challenge period mb_release_settlement(): funds released to merchant
   d. In case of dispute: buyer can mb_dispute(), merchant can mb_force_release(), or buyer can mb_resolve_dispute() to submit fraud proof
```

---

## 4. Shared Infrastructure

### 4.1 ignite-pay-core

Core protocol library, providing:

| Module | Function |
|------|------|
| `identity` | DID generation, DID Document construction, identity persistence, DID signature verification |
| `didcomm` | DIDComm message constructors (15 message types), JWE encryption/decryption, Agent creation |
| `types` | Shared types: PaymentRequest, MerchantListEntry, VerifiableCredential, RiskControlDecision |
| `list_store` | Whitelist/blacklist management (sled + IPFS sync), risk control decisions |
| `vc` | Verifiable Credential issuance and verification |
| `ipfs` | IPFS upload/download abstraction layer |
| `audit_merkle` | SHA-256 Merkle tree audit log |
| `log_crypto` / `log_chunk` / `log_sync` | E2EE audit log (encrypt -> Zstd compress -> IPFS sync) |

### 4.2 ignite-pay-mb-sdk

MagicBlock payment channel SDK, providing:

| Module | Function |
|------|------|
| `pda` | PDA derivation: `derive_global_state_pda`, `derive_global_vault_pda`, `derive_channel_pda`, `derive_settlement_pda` |
| `merkle` | Sum-Merkle Tree (each node stores hash + sum): `build_sum_merkle_tree`, `MerkleProof` (sibling hashes + sums) |
| `signing` | Voucher signing: `sign_voucher(channel_id, seq, amount, sk)` -> `(msg_hash, sig)`; Settlement signing: `sign_settlement`, `build_settlement_message`; Signature verification: `verify_signature` |
| `transaction` | 11 transaction builders: `build_initialize_global_tx`, `build_deposit_tx`, `build_initialize_channel_tx`, `build_update_spending_cap_tx`, `build_settle_batch_tx`, `build_optimistic_settle_tx`, `build_dispute_tx`, `build_resolve_dispute_tx`, `build_release_settlement_tx`, `build_force_release_tx`, `build_withdraw_tx` |

**On-chain Account Structures:**

| Account | Size | Fields |
|------|------|------|
| GlobalState | 57 bytes | `buyer`, `total_deposited`, `total_allocated`, `bump` |
| Channel | 113 bytes | `buyer`, `merchant`, `spending_cap`, `settled_amount`, `nonce`, `challenge_period`, `dispute_period`, `bump` |
| SettlementEscrow | 132 bytes | `channel`, `merchant`, `amount`, `merkle_root`, `nonce`, `created_at`, `claimed`, `disputed`, `optimistic`, `bump` |

**On-chain Instructions:**

| Instruction | Signer | Description |
|------|--------|------|
| `initialize_global` | buyer | Create GlobalState + GlobalVault PDA |
| `deposit` | buyer | Transfer SOL to GlobalVault |
| `initialize_channel` | buyer | Create payment channel, lock spending_cap |
| `update_spending_cap` | buyer | Adjust channel spending cap |
| `settle_batch` | merchant | Dual-signature batch settlement (Ed25519 instruction introspection) |
| `optimistic_settle` | merchant | Merchant-only signature optimistic settlement (requires challenge_period > 0) |
| `release_settlement` | merchant | Release funds after challenge period |
| `dispute` | buyer | Freeze Escrow (within challenge window) |
| `force_release` | merchant | Force release after dispute period |
| `resolve_dispute` | buyer | Fraud proof (Sum-Merkle Proof) |
| `withdraw` | buyer | Withdraw unallocated funds |

### 4.3 DIDComm Message Types

| Message | Direction | Purpose |
|------|------|------|
| `out-of-band/2.0/invitation` | MCP -> User | QR pairing invitation |
| `ignite-pay/1.0/connection-request` | User -> MCP | Establish connection |
| `ignite-pay/1.0/connection-response` | MCP -> User | Connection confirmation |
| `ignite-pay/1.0/payment-auth-request` | MCP -> User | Request payment authorization |
| `ignite-pay/1.0/payment-auth-response` | User -> MCP | Authorization response (with Session Key) |
| `ignite-pay/1.0/channel-payment-request` | — | MagicBlock Voucher payment request |
| `ignite-pay/1.0/channel-payment-confirm` | — | Voucher payment confirmation (with signature) -> push to merchant |
| `ignite-pay/1.0/list-sync-notification` | MCP -> User | Whitelist/blacklist update |
| `coordinate-mediation/2.0/*` | Bidirectional | Mediator protocol (mediate-request, keylist-update) |
| `ignite-pay/1.0/ws-challenge-response` | Bidirectional | WS authentication challenge |
| `messagepickup/3.0/*` | Bidirectional | Message pickup protocol |

### 4.4 Mediator REST API

| Endpoint | Method | Purpose |
|------|------|------|
| `/v1/auth/challenge` | GET | Get authentication nonce |
| `/v1/auth/token` | POST | Exchange signature for JWT |
| `/v1/sync/list` | GET | Pull message list (cursor-based pagination) |
| `/v1/sync/messages/{id}` | GET | Get single message |
| `/v1/agents/{id}/command` | POST | Send encrypted command |
| `/v1/agents/bind` | POST | Bind Agent DID |
| `/v1/devices/register-token` | POST | Register push channel (FCM token or websocket) |

### 4.5 MagicBlock Payment Channel Architecture

**Three-Layer Architecture:**

| Layer | Description |
|------|------|
| L1 (Solana) | Channel creation, fund locking, signature verification, final settlement |
| ER (MagicBlock) | High-speed state transitions (<50ms latency, gasless), records each Voucher |
| Off-chain Fraud Layer | Challenge window dispute resolution, based on Merkle Proof |

**Security Model (Triple Protection):**

| Protection | Description |
|------|------|
| Spending Cap | `settled_amount + total_amount <= spending_cap` (on-chain check) |
| Balance Check | `total_amount <= vault.lamports` (actual balance) |
| Dual Signature | Ed25519 instruction introspection verifies buyer + merchant signatures |

**Global Vault Design:** Each Buyer has one global Vault (`GlobalVault PDA`), with `total_allocated` tracking the sum of all channel spending caps, preventing over-allocation.

---

## 5. Dual-App Comparison

| Dimension | User App (Sentinel) | Merchant App (Merchant) |
|------|-------------------|-------------------|
| **Core Role** | Payment authorization guardian | Payment collection tool |
| **DID Count** | 1 (shared for communication + transactions) | 2 (MB channel Keypair + DIDComm DID) |
| **Message Direction** | Receive auth-request -> Send auth-response | Receive payment-confirm -> Confirm order |
| **QR Interaction** | Scan code (pair MCP / pay) | Generate code (collect payment) |
| **On-chain Operations** | Session Key registration/revocation, MB channel management | MB settlement/release/dispute handling |
| **Push Trigger** | MCP payment request | Payment confirmation notification |
| **Unique Features** | Whitelist/blacklist policies, external wallet signing | Voice notification, order management |
| **UI Language** | English | Chinese |
| **Screen Count** | 16 | 11 |
| **Rust Modules** | simple + identity + auth + session + ws_client + voucher_store + log_store (7) | merchant + merchant_didcomm + voucher_store + settlement_store (4) |
| **Bridge Functions** | 30+ | 24 |

---

## 6. Design System

Both Apps share the same Dark Glassmorphism design language:

| Token | Value | Purpose |
|-------|-----|------|
| Background | `#0A0A14` | Page background |
| Surface | `#12121F` ~ `#22223A` | Cards, input fields, borders |
| Text Primary | `#F0F0F8` | Headings, amounts |
| Text Secondary | `#7A7A96` | Descriptions |
| Neon Cyan | `#00F5FF` | Primary accent color, button gradients |
| Purple | `#8B5CF6` | Secondary accent color |
| Success | `#00E676` | Confirmed, connected |
| Pending | `#FFB300` | Pending |
| Danger | `#FF5252` | Failed, closed, disconnected |

Fonts: Inter (body text) + JetBrains Mono (numeric values, DIDs, code).

Shared components: `BackButtonGlass`, `PageHeader`, `SettingsTile`, `SectionLabel`, `glassDecoration()`.

---

## 7. Data Models

### 7.1 Order (PaymentOrder)

```
State transitions: pending -> confirmed / failed / expired

Fields:
  orderId        String      UUID v4
  merchantDid    String      did:ignite:...
  amount         BigInt      lamports (1 USDC = 1_000_000_000)
  description    String      Optional description
  merchantPubkey String      Merchant MB Ed25519 public key (base58)
  status         String      "pending" | "confirmed" | "failed" | "expired"
  createdAt      int         Unix seconds
  confirmedAt    int?        Unix seconds (confirmed only)
  channelId      String?     Channel PDA (confirmed only)
  voucherSeq     BigInt?     Voucher sequence number (confirmed only)
  buyerSig       String?     Buyer Voucher signature (confirmed only)
```

### 7.2 Channel (ChannelAccount)

```
Fields:
  buyer            Pubkey      Buyer public key
  merchant         Pubkey      Merchant public key
  spending_cap     u64         Spending cap (lamports)
  settled_amount   u64         Settled amount (lamports)
  nonce            u64         Current batch nonce
  challenge_period i64         Challenge period (seconds)
  dispute_period   i64         Dispute period (seconds)
```

### 7.3 Global State (GlobalStateAccount)

```
Fields:
  buyer            Pubkey      Buyer public key
  total_deposited  u64         Total deposited amount (lamports)
  total_allocated  u64         Total allocated amount (sum of all channel spending caps)
```

### 7.4 Settlement Escrow (SettlementEscrowAccount)

```
Fields:
  channel          Pubkey      Channel PDA
  merchant         Pubkey      Merchant public key
  amount           u64         Settlement amount
  merkle_root      [u8; 32]    Merkle Sum Tree root hash
  nonce            u64         Batch nonce
  created_at       i64         Creation timestamp
  claimed          bool        Whether released
  disputed         bool        Whether disputed
  optimistic       bool        Whether optimistic settlement
```

### 7.5 DIDComm Message (DecryptedMessage)

```
Fields after merchant-side decryption:
  msgType      String      Message type URI
  orderId      String?     Associated order ID
  channelId    String?     Channel PDA
  voucherSeq   BigInt?     Voucher sequence number
  amount       BigInt?     Confirmed amount
  buyerSig     String?     Buyer Voucher signature (base58)
  authorized   bool?       Authorization status
  rawBody      String      Raw JSON
```

---

## 8. Push Notification Architecture

```
                    ┌───────────────────────┐
                    │      Mediator         │
                    │   (Message Relay +    │
                    │    Push Service)      │
                    └─────┬──────────┬──────┘
                          │          │
              ┌───────────┘          └───────────┐
              │                                  │
        zh_CN Users                         Non-zh_CN Users
              │                                  │
     WebSocket Persistent                  FCM Push Notifications
     Connection                      (SIGNAL -> Pull JWE)
     (Direct JWE Reception)                │
              │                                  │
              └──────────┬───────────────────────┘
                         │
                    pull_messages()
                    decrypt_message()
                    Confirm Order / Authorize Payment
```

**WS Flow** (domestic/China users): Connect -> identify -> continuous listening -> receive JWE -> decrypt and process directly

**FCM Flow** (international users): SIGNAL notification -> `onSignalReceived` -> pull_messages to fetch -> decrypt and process

**Common aspects**:
- On first connection: authenticate -> pull offline messages
- After WS disconnection: pull offline messages first, then reconnect (3-second delay)
- When FCM notification received in foreground: display local notification (title "Payment Received")

---

## 9. Security Model

| Security Measure | Description |
|----------|------|
| DID Identity | Ed25519 signing key + X25519 key agreement, locally stored in encrypted sled database |
| Message Encryption | DIDComm authcrypt (JWE), end-to-end encrypted, Mediator cannot read plaintext |
| Mediator Authentication | Challenge-response: nonce -> Ed25519 signature -> JWT token |
| Session Key | Temporary key registered on-chain, validity period and amount limits, revocable |
| Whitelist/Blacklist | IPFS-synced merchant lists, risk control decisions (Blocked/AutoApproved/NeedsAuth) |
| Audit Log | Merkle tree + E2EE encryption + IPFS sync, tamper-proof |
| Dual DID Isolation | MB channel Keypair and communication DID are separated, mutually independent |
| MagicBlock Security | Triple protection: spending cap check + Vault balance check + Ed25519 dual-signature introspection |
| Fraud Proof | Sum-Merkle Tree fraud proof, single Voucher + O(log N) sibling nodes sufficient for proof |
| Challenge Window | After settlement, enters challenge_period; buyer can dispute to freeze funds, submit Merkle Proof to resolve dispute |
