# Ignite Pay App — Page & Interaction Specification Document

> A comprehensive review based on the ignite-pay project documentation, Rust API layer, and existing Flutter code.
> Version: V1.0 -> V2.0 feature coverage

---

## 1. Application Overview

Ignite Pay mobile client (codename Sentinel) is the mobile authorization terminal for the AI Agent payment gateway. Core responsibilities:

1. **DID Identity Management** — Generate/import/backup `did:ignite` decentralized identities
2. **MCP Pairing** — Establish DIDComm P2P connections with AI Agents (MCP Servers) via QR codes
3. **Payment Authorization** — Receive X402 payment challenges, user approval (including session key creation)
4. **Risk Control Policies** — Whitelist/blacklist management, merchant limits, auto-pay rules
5. **Messaging** — Send and receive DIDComm encrypted messages relayed through a Mediator
6. **Audit Logs** — Local transaction records + IPFS encrypted sync

### 1.1 Core Architecture Flow

```
AI Agent --X402--> MCP Server --DIDComm JWE--> Mediator --push--> Phone App
                                                                  |
                                                    FCM (overseas) / WebSocket (domestic)
                                                                  |
                                                         HTTPS Pull (message retrieval)
```

### 1.2 Push Channel Strategy

| User Region | Push Method | Description |
|:------------|:------------|:------------|
| Overseas | FCM (Firebase Cloud Messaging) | Standard push |
| Mainland China | WebSocket persistent connection | Direct connection to Mediator WS |
| Universal fallback | HTTPS polling pull | Triggered after any connection is restored |

---

## 2. Page Map

```
┌─────────────────────────────────────────────────────────┐
│                    Sentinel Dashboard                     │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐   │
│  │ Identity  │ │ 🔔Notif.  │ │ Scan &   │ │ Spending  │   │
│  │   Card    │ │  Center   │ │  Pair    │ │  Gauge    │   │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘   │
├──────────┬──────────┬──────────┬──────────┬─────────────┤
│  Vault   │ Policy   │ Messages │Connection│  Settings   │
│& Identity│  Center  │   List   │  Mgmt.   │             │
├──────────┼──────────┼──────────┼──────────┼─────────────┤
│ ·DID Det.│ ·Merchant│ ·Message │ ·MCP Conn│ ·Solana RPC │
│ ·Mnemon. │ ·Limits  │ ·Payment │ ·Mediator│ ·Tree Addr  │
│ ·Key Exp.│ ·AutoPay │ ·ListSync│ ·Push Ch.│ ·Program ID │
│ ·Audit   │ ·White/  │          │ ·FCM Conf│ ·Network    │
│ ·Danger  │  Blacklist│         │          │  Switch     │
├──────────┴──────────┴──────────┴──────────┴─────────────┤
│                  X402 Challenge (full-screen modal)      │
│         ·Merchant info ·Amount ·Slider auth ·List ops   │
├─────────────────────────────────────────────────────────┤
│                  QR Scanner (full-screen modal)          │
│         ·Scan & pair ·Manual invite URL input            │
├─────────────────────────────────────────────────────────┤
│              Channel Topology                            │
│    ·Balance overview ·Node info ·Channel list ·Close/    │
│                                                 settle  │
├─────────────────────────────────────────────────────────┤
│              Transaction History                         │
│        ·Filter (All/Payment/ListSync) ·Tx details       │
├─────────────────────────────────────────────────────────┤
│              Profile                                     │
│     ·DID display ·Display name ·Network info ·Device    │
│      status ·Statistics                                  │
├─────────────────────────────────────────────────────────┤
│              Hub List                                    │
│    ·Hub discovery ·Hub selection ·Channel creation       │
│     parameter configuration                              │
└─────────────────────────────────────────────────────────┘
```

---

## 3. Detailed Page Specifications

### 3.1 Dashboard (Home)

**Route**: `/` (home)
**Priority**: P0 — Must implement

#### 3.1.1 Page Structure

| Area | Component | Data Source | Status |
|:-----|:----------|:-----------|:-------|
| Top bar | Sentinel Logo + network status + notification bell (unread count) | `DidcommService.isConnected` | Implemented |
| DID identity card | DID text + copy + connection status animated dot + pending message count | `DidcommService.did`, `_isConnected` | Implemented |
| Quick navigation | Vault / Policy / Channels / Settings | Static | Implemented (Channels -> ChannelTopologyScreen) |
| Scan & pair button | "Scan MCP QR Code" large button | Triggers QR Scanner | Implemented (supports PaymentQrData and didcomm://) |
| Create channel button | "Create Channel" button | SharedPreferences hub_registry_url + mcp_did | Implemented |
| Spending gauge | Radial progress bar (spent/limit SOL) | `LocalLogStore` aggregation | UI exists, data needs wiring |
| Recent activity | Transaction list (merchant, amount, time, status) | `DidcommService.messages` (real-time) | Implemented, using real-time data |
| Authorization entry | Pending authorization notification banner + "Authorize Payment" button | `DidcommService._pendingAuth` stream | Implemented |

#### 3.1.2 Interaction Flow

```
[First launch] -> Auto-generate DID -> Dashboard display
[Click scan] -> QR Scanner modal -> Scan PaymentQrData or didcomm:// URL -> Pay or connect MCP -> Return to Dashboard
[Push received] -> Pull messages -> Decrypt -> If payment-auth-request -> Show authorization banner
[Click authorize] -> X402 Challenge modal -> Slider confirm -> Create Session Key -> Encrypt response -> Close
[Click notification bell] -> NotificationCenterScreen
[Click Channels] -> ChannelTopologyScreen
[Click Create Channel] -> HubListScreen -> Select Hub -> Create channel
```

#### 3.1.3 Needs Fixing / Wiring

- [ ] Wire spending gauge to `LocalLogStore` actual transaction data
- [x] Activity feed wired to `DidcommService.messages` real-time data
- [ ] Network status (Mainnet/Devnet) read from configuration
- [x] Pending message count calculated from `DidcommService.messages`

---

### 3.2 Vault & Identity

**Route**: push slide-right
**Priority**: P0 — Must implement

#### 3.2.1 Page Structure

| Area | Component | Data Source | Status |
|:-----|:----------|:-----------|:-------|
| DID identity card | Grid gradient background + DID + key type label + copy | `RustLib.api.getDid()` | Implemented |
| Mnemonic phrase | Tap to reveal/hide 12 words + warning banner | `IdentityManager` key derivation | **Hardcoded, needs wiring** |
| Mediator endpoint | WebSocket URL input field + connect/disconnect button | `DidcommService._mediatorWsUrl` | Partially implemented, missing connect button |
| Audit log entry | Event count badge + navigation | `LocalLogStore` count | **Hardcoded, needs wiring** |
| Key export | Export Ed25519 private key (encrypted) | `IdentityManager` | **Missing** |
| Danger zone | "Erase Key Material" button + confirmation dialog | `IdentityManager` destroy | **UI only, needs wiring** |

#### 3.2.2 Interaction Flow

```
[Click mnemonic] -> Confirmation dialog -> Reveal 12 words -> Click again to hide
[Edit Mediator URL] -> Enter new URL -> Click "Connect" -> WS connect + auth + handshake
[Click erase] -> Second confirmation dialog -> Call Rust cleanup -> Return to onboarding
```

#### 3.2.3 Needs Fixing / Wiring

- [ ] Get mnemonic from Rust `IdentityManager` for actual keys
- [ ] Mediator connect button (currently only URL input field, missing connect action)
- [ ] Wire audit log count to `LocalLogStore`
- [ ] Key export functionality
- [ ] Wire key erasure to Rust layer destroy

---

### 3.3 Connection Management — New Page

**Route**: push slide-right (entered from Dashboard or Settings)
**Priority**: P0 — Must implement

#### 3.3.1 Feature Description

Manage the phone's connections to MCP Servers and the Mediator. This is a critical page that is currently missing.

#### 3.3.2 Page Structure

| Area | Component | Data Source |
|:-----|:----------|:-----------|
| Mediator connection | Connection status indicator + WS URL + connect/disconnect button + auth status | `DidcommService._isConnected`, `_authToken` |
| Push channel config | FCM / WebSocket toggle + FCM Token display + registration status | `FcmService._token`, `DidcommService._isChineseUser` |
| Paired MCP list | MCP DID + label + connection time + last active + delete button | `DidcommService._boundAgents` |
| Add MCP | "Scan QR Code" button + "Enter URL Manually" button | QR Scanner / text input |

#### 3.3.3 Interaction Flow

```
[Open page] -> Show current Mediator connection status + paired MCP list
[Connect Mediator] -> Enter WS URL -> Click connect -> 3-phase handshake -> Status update
[Configure push] -> Select FCM or WebSocket -> FCM: request permission + register Token -> WS: show connection status
[Add MCP] -> Scan QR or enter URL -> OOB parse -> Send connection-request -> Add to list
[Delete MCP] -> Confirmation dialog -> Remove binding
```

#### 3.3.4 Rust API Dependencies

| API | Purpose |
|:----|:--------|
| `connectMediator(storagePath, wsUrl)` | Connect to Mediator |
| `disconnectMediator()` | Disconnect |
| `authenticateWithMediator(mediatorUrl, did)` | JWT authentication |
| `registerDeviceToken(mediatorUrl, authToken, fcmToken)` | Register FCM Token |
| `parseOobInvitation(invitationUrl)` | Parse invitation |
| `sendConnectionRequest(...)` | Establish P2P connection |

---

### 3.4 Policy Architect

**Route**: push slide-right
**Priority**: P1 — Core feature

#### 3.4.1 Page Structure

| Area | Component | Data Source | Status |
|:-----|:----------|:-----------|:-------|
| Statistics overview | 2x2 grid: merchant count / auto-pay count / weekly limit / spent | Aggregated calculation | **Hardcoded** |
| Merchant policy list | Expandable cards + auto-pay toggle | Persisted policy data | **Hardcoded** |
| Policy details | DID / per-transaction limit / weekly spending progress bar / expiry date | Persisted policy data | **Hardcoded** |

#### 3.4.2 Interaction Flow

```
[Expand merchant card] -> Show details -> Edit limit / toggle auto-pay / set expiry
[Modify limit] -> Enter SOL amount -> Save to local policy store
[Toggle auto-pay] -> Switch -> Save -> If enabled, set as whitelist auto-authorize
[View progress bar] -> Calculate this week's spending for merchant from LocalLogStore -> Show progress
```

#### 3.4.3 Needs Fixing / Wiring

- [ ] Merchant list from actual connected MCPs or whitelist/blacklist store
- [ ] Limit data persistence (SharedPreferences or SQLite)
- [ ] Spending progress aggregated from `LocalLogStore`
- [ ] Auto-pay toggle wired to whitelist logic

---

### 3.5 Messages — New Page

**Route**: push slide-right
**Priority**: P0 — Must implement

#### 3.5.1 Feature Description

Display all decrypted DIDComm messages, including payment requests, list sync notifications, connection requests, etc.

#### 3.5.2 Page Structure

| Area | Component | Data Source |
|:-----|:----------|:-----------|
| Message list | Reverse-chronological message feed + message type icon + brief info | `DidcommService._messages` |
| Message details | Expand or navigate to new page showing full message content | `DecryptedMessage` fields |
| Filter | Filter by type: All / Payment Requests / List Sync / Connections | Message `msgType` |
| Empty state | "No messages yet" + pull button | — |

#### 3.5.3 Message Types

| Type | Icon | Detail Display |
|:-----|:-----|:---------------|
| `payment-auth-request` | 💳 | Merchant DID + amount + description + "Authorize" button |
| `list-sync-notification` | 📋 | List type + CID + "Sync" button |
| `connection-request` | 🔗 | Counterparty DID + "Accept/Reject" |
| Other | 📨 | rawBody JSON |

#### 3.5.4 Interaction Flow

```
[Open page] -> Show all decrypted messages
[Pull to refresh] -> Call pullMessages + decryptMessage -> Update list
[Click payment message] -> Open X402 Challenge modal
[Click list sync] -> Call Rust IPFS sync -> Update local list
[Filter] -> Switch message type filter
```

---

### 3.6 Settings — New Page

**Route**: push slide-right
**Priority**: P1 — Core feature

#### 3.6.1 Page Structure

| Area | Component | Data Source |
|:-----|:----------|:-----------|
| Solana network | RPC URL input + network (Mainnet/Devnet) toggle | Config file |
| SPL compression config | Tree Address + Tree Authority + DAS Endpoint | Config file |
| Program IDs | State channel program / DID program / session key program display | Hardcoded constants |
| Payment mode | Self-Funded / Sponsored toggle | Config file |
| Mediator config | WS URL + HTTP URL + current status | `DidcommService` |
| Push channel | FCM / WebSocket selection + detection result | `DidcommService` |
| Storage | Clear cache + storage usage display | — |
| About | Version number + open source licenses | — |

#### 3.6.2 Interaction Flow

```
[Switch network] -> Confirmation dialog -> Update RPC URL + reconnect
[Edit config] -> Input -> Save to SharedPreferences -> Reconnect when needed
[Clear cache] -> Confirm -> Clean local storage
```

---

### 3.7 X402 Challenge (Payment Authorization)

**Route**: Full-screen modal (fade transition)
**Priority**: P0 — Implemented, needs refinement

#### 3.7.1 Page Structure

| Area | Component | Data Source | Status |
|:-----|:----------|:-----------|:-------|
| Title bar | Shield icon + "X402 Challenge" + close button | Static | Implemented |
| Merchant card | Merchant DID + verification badge | `AuthRequest.merchantDid` | Implemented |
| Amount display | Large SOL amount text | `AuthRequest.amount` | Implemented |
| Payment reason | Description text | `AuthRequest.description` | Implemented |
| List actions | 6 action choices + label input + max amount | `ListAction` enum | Implemented |
| Slider authorization | Drag slider to confirm | Gesture | Implemented |
| Decline button | "Decline & Block" | Gesture | Implemented |
| Result banner | "Creating session key..." / "Authorized" / Error | Flow state | Implemented |

#### 3.7.2 Authorization Flow (V2.0 — with Session Key)

```
[Received payment-auth-request]
  -> Decrypt message -> Set _pendingAuth -> Dashboard shows notification banner
  -> User clicks -> Open X402 Challenge modal
  -> Show merchant DID + amount + description
  -> User selects list action (optional)
  -> User drags slider to 85%
  -> Trigger _onAuthorize():
     1. Call createSessionKeyForPayment(spendingLimit=amount*10, durationSecs=3600)
     2. Call sendAuthResponse(paymentId, authorized=true, listAction, mcpDid, sessionKeyInfo)
     3. Wait for encrypted response to be sent
  -> Success: Show green result banner -> Close after 1.5s
  -> Failure: Show red error banner
```

#### 3.7.3 Needs Refinement

- [ ] Merchant DID on-chain verification status display (VC verification result)
- [ ] Session key details display (public key, expiry time, limit)
- [ ] Error classification prompts (network error / auth failure / amount exceeds limit)

---

### 3.8 QR Scanner (Scan & Pair)

**Route**: Full-screen modal
**Priority**: P0 — Implemented, needs enhancement

#### 3.8.1 Page Structure

| Area | Component | Status |
|:-----|:----------|:-------|
| Camera preview | MobileScanner + scan area overlay | Implemented |
| Manual input | "Enter Invite URL Manually" button | **Missing** |
| Scan result | Display parsed MCP DID + label + confirm button | **Missing** |

#### 3.8.2 Interaction Flow

```
[Scan successful] -> Parse didcomm:// URL -> OobInvitationData
  -> [New] Show confirmation dialog: MCP DID + label + Mediator URL
  -> User confirms -> Connect Mediator -> Send connection-request
  -> Success: Close Scanner -> Dashboard updates connection status
  -> Failure: Show error + retry

[Manual input] -> Text input field -> Paste didcomm:// URL -> Same flow as above
```

---

### 3.9 Audit Logs

**Route**: push slide-right (entered from Vault)
**Priority**: P2 — Enhancement feature

#### 3.9.1 Page Structure

| Area | Component | Data Source |
|:-----|:----------|:-----------|
| Log list | Reverse-chronological transaction records | `LocalLogStore.recent_transactions(limit)` |
| Log entry | Action type + merchant + amount + time + status badge | `TransactionLog` |
| IPFS sync status | Synced/unsynced count + manual sync button | `LocalLogStore.unsynced_entries()` |
| Search/filter | Filter by action type or date range | Client-side filtering |

#### 3.9.2 Interaction Flow

```
[Open page] -> Load recent transactions from SQLite
[Click sync] -> Call sync_to_ipfs() -> Show progress -> Update sync status
[Pull to refresh] -> Reload
```

---

### 3.10 Onboarding — New Page

**Route**: Displayed on first launch (check if DID exists locally)
**Priority**: P1 — Core feature

#### 3.10.1 Page Structure

| Step | Content | Action |
|:-----|:--------|:-------|
| Welcome | App introduction + "Get Started" button | Next |
| Create identity | "Generate New DID" button + mnemonic display + confirm backup | Generate + confirm |
| Import identity | "Import Mnemonic" 12-word input fields | Restore |
| Mediator config | WS URL input (with default value) + connection test | Next |
| Complete | "Enter Sentinel" button | Navigate to Dashboard |

---

### 3.11 Notification Center (NotificationCenterScreen)

**Route**: push slide-right (entered from Dashboard notification bell)
**Priority**: P1 — Implemented

#### 3.11.1 Page Structure

| Area | Component | Data Source | Status |
|:-----|:----------|:-----------|:-------|
| Title bar | PageHeader "Notifications" | Static | Implemented |
| Mark all as read | Tappable text button | SharedPreferences `read_notification_ids` | Implemented |
| Message list | Notification card list | `DidcommService.messages` filtered to non payment-auth-request | Implemented |
| Notification card | Icon + summary + message type + unread dot + arrow | `DecryptedMsg` | Implemented |
| Detail popup | Dialog: message type, CID, label, description, RAW BODY | `DecryptedMsg` | Implemented |

#### 3.11.2 Read Status Management

- Uses SharedPreferences to store read notification IDs (key: `read_notification_ids`)
- ID generated from `msg.rawBody.hashCode.toString()`
- Unread notifications indicated by NeonCyan border + dot

---

### 3.12 Channel Topology (ChannelTopologyScreen)

**Route**: push slide-right (entered from Dashboard quick navigation Channels)
**Priority**: P1 — Implemented

#### 3.12.1 Page Structure

| Area | Component | Data Source | Status |
|:-----|:----------|:-----------|:-------|
| Title bar | PageHeader "Channel Topology" | Static | Implemented |
| Balance overview card | Total balance (SOL) + open/closed channel count | `ChannelService.totalBalance` | Implemented |
| Local node card | CPU icon + user DID + connection status pulse dot | `DidcommService.did`, `isConnected` | Implemented |
| Channel card list | Hub endpoint + status badge + balance/deposit/sequence/depth | `ChannelService.channels` | Implemented |
| Channel actions | Close (open channel) / Settle (closed channel) buttons | Rust `closeChannel` / `settleChannel` | Implemented |
| Pull to refresh | RefreshIndicator | `_loadChannels()` | Implemented |

#### 3.12.2 Interaction Flow

```
[Open page] -> Load channel list -> Show balance overview + channel list
[Pull to refresh] -> Re-call ChannelService.refreshChannels
[Click Close] -> Confirmation dialog -> Call Rust closeChannel -> Refresh list
[Click Settle] -> Call Rust settleChannel -> Refresh list
```

---

### 3.13 Transaction History (TransactionHistoryScreen)

**Route**: push slide-right
**Priority**: P1 — Implemented

#### 3.13.1 Page Structure

| Area | Component | Data Source | Status |
|:-----|:----------|:-----------|:-------|
| Title bar | PageHeader "Transaction History" | Static | Implemented |
| Filter tabs | All / Payment / List Sync horizontal scroll | `_TxFilter` enum | Implemented |
| Transaction card list | Icon + merchant DID + amount + status badge | `DidcommService.messages` filtered by type | Implemented |
| Transaction detail popup | Dialog: type, Payment ID, merchant, amount, description, RAW BODY | `DecryptedMsg` | Implemented |

#### 3.13.2 Message Type Mapping

| Filter Type | Icon | Color | Filter Condition |
|:------------|:-----|:------|:-----------------|
| Payment | creditCard | Amber | `msgType.contains('payment')` |
| List Sync | listChecks | Purple | `msgType.contains('list-sync')` |
| Other | mail | TextSecondary | Does not contain the above keywords |

---

### 3.14 Profile (ProfileScreen)

**Route**: push slide-right
**Priority**: P1 — Implemented

#### 3.14.1 Page Structure

| Area | Component | Data Source | Status |
|:-----|:----------|:-----------|:-------|
| Avatar | Gradient circle + first two characters of DID | `DidcommService.did` | Implemented |
| DID display | Glass card + DID text + copy button | `DidcommService.did` | Implemented |
| Display name | Editable input field + save | SharedPreferences `display_name` | Implemented |
| Network info | Devnet/Mainnet label | SharedPreferences `network` | Implemented |
| Device status | Connection status dot + Session Key active badge | `DidcommService.isConnected`, `SessionKeyService` | Implemented |
| Statistics card | Channel count / balance / merchant count | `ChannelService`, `SharedPreferences` | Implemented |
| Export DID | OutlinedButton "Export DID Document" | `DidcommService.didDocJson` | Implemented |

---

### 3.15 Hub List (HubListScreen)

**Route**: push slide-right (entered from Dashboard "Create Channel" button)
**Priority**: P1 — Implemented

#### 3.15.1 Page Structure

| Area | Component | Data Source | Status |
|:-----|:----------|:-----------|:-------|
| Hub list | Fetched from Hub Registry API | `GET /v1/hubs` | Implemented |
| Hub card | Name, description, status, fee rate, liquidity, latency | Hub Registry | Implemented |
| Channel creation | Select Hub -> Enter parameters -> Create channel | `sendCreateChannelRequest` | Implemented |

---

## 4. Data Models

### 4.1 Core State (Global state managed by DidcommService)

```dart
class DidcommState {
  // Identity
  String did;                    // did:ignite:z...
  String didDocJson;             // DID Document JSON

  // Connection
  bool isConnected;              // Mediator WS connection status
  String? authToken;             // JWT Token
  String mediatorWsUrl;          // Mediator WebSocket URL
  String mediatorHttpUrl;        // Mediator HTTP URL

  // Paired MCPs
  List<McpConnection> boundAgents;  // Connected MCP list

  // Messages
  List<DecryptedMessage> messages;  // Decrypted messages
  AuthRequest? pendingAuth;         // Pending authorization

  // Push
  PushChannel pushChannel;       // fcm | websocket
  String? fcmToken;              // FCM Token
}
```

### 4.2 McpConnection (MCP Connection Record)

```dart
class McpConnection {
  String mcpDid;                 // MCP Server's DID
  String label;                  // Display name
  DateTime connectedAt;          // Connection time
  DateTime? lastActive;          // Last active time
  String mediatorWsUrl;          // Mediator used for connection
}
```

### 4.3 Policy (Merchant Policy)

```dart
class MerchantPolicy {
  String merchantDid;            // Merchant DID
  String label;                  // Display name
  bool autoPay;                  // Auto-pay toggle
  double singleLimit;            // Per-transaction limit (SOL)
  double weeklyCap;              // Weekly limit (SOL)
  DateTime? expiryDate;          // Policy expiry time
  ListAction listAction;         // Whitelist/blacklist status
  String? listLabel;             // List label
  double? listMaxAmount;         // List maximum amount
}
```

### 4.4 TransactionLog (Transaction Log)

```dart
class TransactionLog {
  String id;                     // Log ID
  String action;                 // sign_payment | key_derive | ...
  String? merchantDid;           // Merchant DID
  double? amount;                // Amount (SOL)
  DateTime timestamp;            // Timestamp
  String status;                 // success | pending | failed
  bool synced;                   // Whether synced to IPFS
}
```

---

## 5. Rust API Integration Checklist

### 5.1 Integrated (Available)

| Rust API | Dart Call Location | Description |
|:---------|:-------------------|:------------|
| `initializeIdentity` | `DidcommService.initialize()` | DID generation/loading |
| `getDid` | `DidcommService.initialize()` | Get DID |
| `connectMediator` | `DidcommService.connectToMediator()` | WS connection |
| `disconnectMediator` | `DidcommService.disconnect()` | Disconnect |
| `authenticateWithMediator` | `DidcommService.connectToMediator()` | JWT authentication |
| `pullMessages` | `DidcommService._pullAndDecryptMessages()` | Pull messages |
| `decryptMessage` | `DidcommService._pullAndDecryptMessages()` | Decrypt |
| `sendAuthResponse` | `DidcommService.sendAuthResponse()` | V1.0 authorization |
| `createSessionKeyForPayment` | `ChallengeScreen._onAuthorize()` | Create session key |
| `registerDeviceToken` | `DidcommService.connectToMediator()` | Register FCM |
| `parseOobInvitation` | `DidcommService.parseInvitationAndConnect()` | Parse invitation |
| `sendConnectionRequest` | `DidcommService.parseInvitationAndConnect()` | P2P connection |
| `parsePaymentQr` | `ChannelService.parsePaymentQr()` | Parse payment QR code |
| `listChannels` | `ChannelService.refreshChannels()` | List user channels |
| `channelPay` | `ChannelService.channelPay()` | State channel payment |
| `openChannel` | `ChannelService.openChannel()` | Open state channel |
| `closeChannel` | `ChannelTopologyScreen._closeChannel()` | Close channel |
| `settleChannel` | `ChannelTopologyScreen._settleChannel()` | Settle channel |

### 5.2 Not Yet Integrated (Needs Addition)

| Rust API | Target Page | Purpose |
|:---------|:------------|:--------|
| `createSessionKey` | Settings / Challenge | Advanced session key creation (specify scopes) |
| `createAndRegisterSessionKey` | Settings / Challenge | On-chain session key registration |
| `signPayment` | Challenge | Real payment signing (currently mock) |
| `LocalLogStore.record_transaction` | After Challenge auth success | Record transaction |
| `LocalLogStore.recent_transactions` | Dashboard / Audit Logs | Transaction history |
| `LocalLogStore.unsynced_entries` | Audit Logs | Unsynced logs |
| `LocalLogStore.sync_to_ipfs` | Audit Logs | IPFS sync |
| `LocalLogStore.restore_from_ipfs` | Audit Logs | IPFS restore |

### 5.3 Rust APIs to Be Created

| Requirement | Description |
|:------------|:------------|
| `exportMnemonicPhrase(storagePath) -> Vec<String>` | Export mnemonic phrase |
| `importMnemonicPhrase(words) -> DidInfo` | Restore from mnemonic phrase |
| `eraseAllKeyMaterial(storagePath)` | Secure erasure |
| `getBoundAgents(storagePath) -> Vec<BoundAgent>` | Get paired MCP list |
| `removeBoundAgent(storagePath, agentDid)` | Delete MCP binding |
| `getSessionKeyInfo(storagePath) -> Vec<SessionKeyInfo>` | View active session keys |
| `revokeSessionKey(storagePath, sessionPda)` | Revoke session key |

---

## 6. Key Interaction Flows

### 6.1 First Launch Flow

```
App launches
  -> Check if DID exists locally (sled DB under storagePath)
  -> [Does not exist] -> Onboarding wizard
     -> Step 1: Welcome introduction
     -> Step 2: Generate DID + show mnemonic + confirm backup
     -> Step 3: Configure Mediator URL (default wss://relay.ignite.did)
     -> Step 4: Connection test -> Success -> Enter Dashboard
  -> [Already exists] -> Load DID directly -> Dashboard
```

### 6.2 MCP Pairing Flow

```
Dashboard click "Scan MCP QR Code"
  -> QR Scanner modal opens
  -> Scan didcomm://?_oob=<base64url> URL
  -> [New] Show confirmation dialog:
     - MCP DID: did:ignite:z...
     - Label: "My Agent"
     - Mediator: wss://relay.ignite.did
  -> User confirms
  -> Rust parseOobInvitation() -> OobInvitationData
  -> Check if Mediator is connected; if not, connect first
  -> Rust sendConnectionRequest(storagePath, mcpDid, mcpDidDocJson, mediatorWsUrl, pushChannel, fcmToken?)
  -> Wait for connection confirmation
  -> Success: Save McpConnection -> Dashboard updates -> Close Scanner
  -> Failure: Show error message + retry option
```

### 6.3 Payment Authorization Flow (Complete)

```
[Mediator push / WS message / HTTPS pull]
  -> DidcommService receives JWE envelope
  -> Rust decryptMessage() -> DecryptedMessage
  -> msgType == "payment-auth-request"
  -> Set _pendingAuth -> Dashboard shows authorization banner

[User clicks "Authorize Payment"]
  -> X402 Challenge modal opens
  -> Display: merchant DID / amount (SOL) / description
  -> User selects list action (optional):
     - This time only / Whitelist / Blacklist / ...
     - Enter label (optional)
     - Enter max amount (optional)
  -> User drags slider to 85% -> Trigger authorization

  _onAuthorize():
    1. Rust createSessionKeyForPayment(spendingLimit, durationSecs)
       -> SessionKeyInfo(ephemeralPubkey, ephememalSecretKey, expiresAt, ...)
    2. [New] Rust LocalLogStore.record_transaction(...)
    3. Rust sendAuthResponse(paymentId, authorized=true, listAction, mcpDid, sessionKeyInfo, ...)
       -> Encrypt to JWE -> Send to Mediator via WS/HTTP
    4. Wait for send confirmation
  -> Success: Green result banner -> Close after 1.5s
  -> Failure: Red error banner -> Retry option
```

### 6.4 Mediator Connection Flow

```
User enters Mediator WS URL (or uses default)
  -> Click "Connect"
  -> Rust connectMediator(storagePath, wsUrl)
    -> Phase 0: Receive ws-challenge -> DID sign -> Send ws-challenge-response -> Receive ws-auth-ok
    -> Phase A: Send mediate-request -> Receive mediate-grant -> Send keylist-update -> Send peer-introduction
    -> Enter bidirectional loop
  -> Rust authenticateWithMediator(httpUrl, did) -> JWT Token
  -> Register based on push channel:
     - FCM: Rust registerDeviceToken(mediatorUrl, token, fcmToken)
     - WS: DidcommService._initWebSocketChannel()
  -> Connection successful -> Status update -> Pull offline messages
```

---

## 7. Navigation Structure

### 7.1 Bottom Navigation Bar (Suggested Addition)

```
┌────────────────────────────────────────────┐
│  🏠 Home   │  📨 Messages  │  ⚙️ Settings  │
└────────────────────────────────────────────┘
```

| Tab | Page | Description |
|:----|:-----|:------------|
| Home | Dashboard | Main dashboard |
| Messages | Messages | Message center |
| Settings | Settings | Settings (includes Vault, Policy entry points) |

### 7.2 Page Hierarchy

```
MaterialApp
├── BottomNavigationBar
│   ├── Tab 0: SentinelDashboard (Home)
│   │   ├── -> VaultIdentityScreen (push)
│   │   │   └── -> AuditLogsPage (push)
│   │   ├── -> PolicyArchitectScreen (push)
│   │   ├── -> ConnectionManagementScreen (push)
│   │   ├── -> ChannelTopologyScreen (push) — Channel topology
│   │   ├── -> NotificationCenterScreen (push) — Notification center
│   │   ├── -> ProfileScreen (push) — Profile
│   │   ├── -> TransactionHistoryScreen (push) — Transaction history
│   │   ├── -> HubListScreen (push) — Hub list / Create channel
│   │   ├── -> QrPaymentScreen (push) — Channel payment confirmation
│   │   ├── -> showQrScanner (modal)
│   │   └── -> showX402Challenge (modal)
│   ├── Tab 1: MessagesScreen (Messages)
│   │   └── -> MessageDetail (push)
│   └── Tab 2: SettingsScreen (Settings)
│       └── -> ConnectionManagementScreen (push)
├── OnboardingScreen (conditionally displayed on first launch)
```

---

## 8. Design Style

### 8.1 Theme

- **Style**: Dark glassmorphism
- **Primary color**: Neon Cyan (#00F5FF)
- **Warning color**: Amber (#FFB800)
- **Success color**: Emerald Green (#00FF88)
- **Error color**: Rose (#FF3366)
- **Background**: Dark gradient (#0A0E17 -> #141B2D)
- **Cards**: Semi-transparent blur + subtle border (rgba(255,255,255,0.05))

### 8.2 Typography

- **UI text**: Inter (Google Fonts)
- **Monospace data**: JetBrains Mono (DID, amounts, addresses)
- **Headings**: Inter Bold

### 8.3 Icons

- Lucide Icons used throughout the application

---

## 9. Implementation Priority

### Phase 1 — Core Feature Integration (P0)

| Task | Page | Description |
|:-----|:-----|:------------|
| Fix sled read-only path | Dashboard | Use application internal storage path |
| Add Connection Management page | New page | MCP pairing management + Mediator connection + push config |
| Add Messages page | New page | Message list + details + filtering |
| Add Settings page | New page | Solana / Mediator / push configuration |
| Enhance QR Scanner | QR Scanner | Scan confirmation + manual input |
| Wire audit logs | Audit Logs | LocalLogStore integration |
| Wire transaction history | Dashboard | Activity feed with real data |
| Add new Rust APIs | Rust | Export mnemonic / erase keys / get bound list |

### Phase 2 — Feature Completion (P1)

| Task | Page | Description |
|:-----|:-----|:------------|
| Add Onboarding wizard | New page | First launch flow |
| Wire policy management | Policy | Persistence + real data |
| Wire mnemonic phrase | Vault | Real key derivation |
| Wire spending gauge | Dashboard | Real spending data |
| Bottom navigation bar | Global | Home / Messages / Settings |

### Phase 3 — Advanced Features (P2)

| Task | Page | Description |
|:-----|:-----|:------------|
| Session key management UI | Settings | View/revoke active session keys |
| IPFS audit sync | Audit Logs | Sync/restore |
| Multi-language support | Global | Chinese/English |
| Biometric authentication | Vault | Local security layer |

---

## 10. Known Issues

| Issue | Impact | Fix Plan |
|:------|:-------|:---------|
| sled path `./phone_data` is read-only | App cannot start | Use `path_provider` to get application internal directory |
| FCM unavailable on emulator | Push notifications not working | Use WS push on emulator only, document the limitation |
| DidcommService has duplicated connection logic | Inconsistent with Rust | Unify through Rust `sendConnectionRequest` |
| Policy data is entirely hardcoded | Cannot persist | Add local SQLite policy table |
| Missing error prompt UI | Users cannot perceive errors | Add global SnackBar error prompts |
