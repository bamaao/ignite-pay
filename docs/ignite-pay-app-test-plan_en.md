# Ignite Pay App — Functional Test Document

> Version: V2.0 | Coverage: Identity Management, MCP Pairing, Payment Authorization, Session Keys, Messaging, Risk Control Policies, Settings Management
> Test Environment: Android Emulator (API 36, x86_64) + Solana Devnet

---

## 1. Test Overview

### 1.1 Business Module Breakdown

| ID | Module | Related Screens | Priority |
|:---|:-------|:---------------|:---------|
| M01 | First Launch & Identity Creation | OnboardingScreen | P0 |
| M02 | DID Identity Management | VaultScreen | P1 |
| M03 | MCP Pairing Connection | QrScannerScreen, ConnectionScreen | P0 |
| M04 | Payment Authorization (X402 Challenge) | ChallengeScreen | P0 |
| M05 | Session Key Management | SessionKeysScreen, ChallengeScreen | P0 |
| M06 | Messaging | MessagesScreen | P1 |
| M07 | Risk Control Policies | PolicyScreen | P2 |
| M08 | App Settings | SettingsScreen | P2 |
| M09 | Push Channel | FcmService / WebSocket | P1 |
| M10 | Deep Link Callback | AndroidManifest, MainNavigator | P1 |

### 1.2 Prerequisites

| Condition | Description |
|:----------|:------------|
| Solana Devnet Reachable | RPC URL `https://api.devnet.solana.com` responds normally |
| Mediator Service Running | Local or remote Mediator WebSocket service connectable |
| MCP Server Running | MCP service capable of generating `didcomm://?_oob=...` QR codes |
| Phantom/Solflare | At least one Solana wallet installed (for Deep Link testing) |
| FCM Available | Google Play Services functioning normally (for FCM push testing) |

---

## 2. M01 — First Launch & Identity Creation

### 2.1 Business Rules

| Rule ID | Constraint |
|:--------|:-----------|
| R01-01 | First launch (DID is empty) -> Show Onboarding wizard, do not show Dashboard |
| R01-02 | Non-first launch (DID already exists) -> Go directly to Dashboard |
| R01-03 | DID format must be `did:ignite:z6Mk...` (Ed25519 multibase encoding) |
| R01-04 | Identity creation cannot be skipped; Mediator connection can be skipped |
| R01-05 | Mediator connection failure does not block wizard completion |

### 2.2 Test Cases

#### TC-M01-01 First Launch — Complete Wizard Flow

```
Prerequisite: Clear app data (Settings -> Clear Cache or uninstall and reinstall)

Steps:
  1. Launch the app
  2. Verify: WelcomeStep is displayed ("Sentinel — Your AI Payment Guardian")
  3. Click "Get Started"
  4. Verify: Enters CreateIdentityStep, displays "Generate DID" button
  5. Click "Generate DID"
  6. Verify: Button changes to a loading spinner
  7. Wait for DID generation to complete
  8. Verify: DID card is displayed, DID format is "did:ignite:z6Mk...", 32+ characters
  9. Click "Continue"
  10. Verify: Enters MediatorConfigStep, WS URL defaults to "wss://relay.ignite.did"
  11. Click "Skip"
  12. Verify: "You're all set!" completion page is displayed
  13. Click "Enter Sentinel"
  14. Verify: Enters Dashboard home, DID card shows the generated DID

Expected Result: DID is successfully created and persisted; relaunching goes directly to Dashboard
```

#### TC-M01-02 Repeat Launch Skips Wizard

```
Prerequisite: TC-M01-01 has been completed

Steps:
  1. Fully exit the app
  2. Relaunch the app

Expected Result: Dashboard is displayed directly without the Onboarding wizard; DID card shows the created identity
```

#### TC-M01-03 DID Creation Failure Handling

```
Prerequisite: Clear app data; induce a storage exception (e.g., disk full)

Steps:
  1. Launch the app -> Get Started -> Generate DID

Expected Result: A red SnackBar error message is displayed; retry is available
```

#### TC-M01-04 Mediator Connection Success

```
Prerequisite: Identity creation completed; Mediator service is reachable

Steps:
  1. In MediatorConfigStep, enter the correct WS URL
  2. Click "Connect & Continue"
  3. Verify: Button shows a loading spinner
  4. Wait for connection to succeed

Expected Result: "You're all set!" is displayed; Dashboard connection status is green
```

#### TC-M01-05 Mediator Connection Failure Can Be Skipped

```
Prerequisite: Identity creation completed; Mediator service is unreachable

Steps:
  1. In MediatorConfigStep, enter an incorrect WS URL
  2. Click "Connect & Continue"
  3. Wait for connection timeout/failure

Expected Result: Error message is displayed; retry is available or click "Skip" to continue completing the wizard
```

### 2.3 Interaction Flow Diagram

```
App Launch
    |
    +-- DID exists? --Yes--> Dashboard
    |
    +-- No
        |
        v
    WelcomeStep --Get Started--> CreateIdentityStep
                                      |
                              Generate DID
                                      |
                            +--Success--+--Error--> SnackBar + Retry
                            |
                            v
                     MediatorConfigStep
                      |              |
              Connect & Continue    Skip
                      |              |
               +--Success--+        |
               |           |        |
             Error      OnboardingComplete <---+
               |           |
            SnackBar    Enter Sentinel
               |           |
            Retry/Skip    Dashboard
```

---

## 3. M02 — DID Identity Management

### 3.1 Business Rules

| Rule ID | Constraint |
|:--------|:-----------|
| R02-01 | DID can be copied to clipboard |
| R02-02 | Mnemonic phrase is hidden by default; click to show, click again to hide |
| R02-03 | "Erase Key Material" is a demo feature and does not actually delete |
| R02-04 | Audit logs display local operation records (signing, key derivation, etc.) |

### 3.2 Test Cases

#### TC-M02-01 View DID Identity

```
Prerequisite: Identity creation completed

Steps:
  1. Dashboard -> "Vault" shortcut, or Settings -> "Vault & Identity"
  2. Verify: Vault page is displayed
  3. Verify: Identity Hero Card shows the full DID, format "did:ignite:z6Mk..."
  4. Click the DID text
  5. Verify: "Copied to clipboard" SnackBar is displayed

Expected Result: DID is correctly displayed and can be copied
```

#### TC-M02-02 View Mnemonic Phrase

```
Prerequisite: Navigate to Vault page

Steps:
  1. Find the "Secret Phrase" tile
  2. Verify: Content is masked (displays "••••••••..." or similar)
  3. Click the eye icon
  4. Verify: Displays 12 words (orbit, glacier, velvet, phoenix, tundra, mirror, beacon, labyrinth, cascade, ember, zenith, prism)
  5. Click the eye icon again
  6. Verify: Content is masked again

Expected Result: Mnemonic phrase can toggle between show/hide
```

#### TC-M02-03 View Audit Logs

```
Prerequisite: Navigate to Vault page

Steps:
  1. Click the "Audit Logs" tile
  2. Verify: Enters the audit log page
  3. Verify: Displays a list of operation records (sign_payment, key_derive, etc.)
  4. Verify: Each record contains a timestamp and operation type

Expected Result: Audit logs are correctly displayed
```

---

## 4. M03 — MCP Pairing Connection

### 4.1 Business Rules

| Rule ID | Constraint |
|:--------|:-----------|
| R03-01 | QR code must start with `didcomm://`, otherwise show format error |
| R03-02 | After successful scan, automatically parse the OOB invitation and send connection-request |
| R03-03 | Connection request is sent via WS channel (when connected) or HTTPS (when not connected) |
| R03-04 | Mediator connection status is reflected in real-time on the Connection page |
| R03-05 | MCP list is extracted from existing messages with deduplicated merchant DIDs |

### 4.2 Test Cases

#### TC-M03-01 QR Code Pairing Success

```
Prerequisite: Mediator connected; MCP Server running and displaying QR code

Steps:
  1. Dashboard -> Click "Scan MCP QR Code" / "Pair New MCP"
  2. Verify: QR scanning screen opens, showing 260x260 scan frame with corner indicators
  3. Aim at MCP QR code (format didcomm://?_oob=<base64url>)
  4. Verify: Scan succeeds, screen automatically closes
  5. Verify: Returns to Dashboard, SnackBar shows "Connected to MCP: did:ignite:..."

Expected Result: MCP successfully paired; MCP DID is visible on the Connection page
```

#### TC-M03-02 Invalid QR Code

```
Prerequisite: QR scanning screen is open

Steps:
  1. Scan a non-didcomm:// QR code (e.g., URL, text)

Expected Result: Red error message "Invalid invitation URL" is displayed, or no response
```

#### TC-M03-03 Mediator Connection Management

```
Prerequisite: Navigate to Settings -> Connections

Steps:
  1. Verify: Mediator Card shows current connection status (Connected/Disconnected)
  2. Enter a new WS URL
  3. Click Connect
  4. Verify: Button shows loading; after successful connection, status changes to Connected (green dot)
  5. Click Disconnect
  6. Verify: Status changes to Disconnected (red dot)

Expected Result: Mediator connection can be properly established and disconnected
```

#### TC-M03-04 Push Channel Display

```
Prerequisite: Mediator connected

Steps:
  1. Navigate to Settings -> Connections
  2. View the Push Channel Card

Expected Result:
  - Chinese users (zh_CN locale): Display "WebSocket" badge
  - Overseas users: Display "FCM" badge
```

#### TC-M03-05 Scanning While Not Connected

```
Prerequisite: Mediator not connected

Steps:
  1. Click "Scan MCP QR Code"
  2. Scan a valid QR code

Expected Result: System automatically connects to Mediator first, then sends the connection request; or prompts to configure Mediator first
```

### 4.3 Interaction Flow Diagram

```
Dashboard --Scan QR--> QrScannerScreen
                            |
                    Detected didcomm:// URL
                            |
                    +--Valid format--+--Invalid format--> Red prompt
                    |
                    v
            parseOobInvitation()
                    |
                    v
            sendConnectionRequest()
              |              |
         WS connected      WS not connected
              |              |
         WS sends JWE    HTTP POST sends
              |              |
              +------+-------+
                     |
              +--Success--+--Error--> SnackBar
              |
              v
         Return to Dashboard
         SnackBar: "Connected to MCP: ..."
```

---

## 5. M04 — Payment Authorization (X402 Challenge)

### 5.1 Business Rules

| Rule ID | Constraint |
|:--------|:-----------|
| R04-01 | Upon receiving a `payment-auth-request` message, pop up the Challenge full-screen dialog |
| R04-02 | Amount displayed in SOL (lamports / 10^9); large amounts rounded, small amounts keep 4 significant decimal places |
| R04-03 | Slide-to-authorize requires dragging to 85% position to trigger, otherwise snaps back |
| R04-04 | When authorizing, automatically check for an active session key and reuse it if available |
| R04-05 | When no active key exists, pop up the signing method selector (Built-in / Deep Link / Mobile Wallet) |
| R04-06 | Spending Limit defaults to 10x the payment amount |
| R04-07 | Session Key validity period defaults to 3600 seconds (1 hour) |
| R04-08 | List Action defaults to "none" (this time only) |
| R04-09 | Selecting "Whitelist" or "Blacklist" reveals a Label input field |
| R04-10 | Selecting "Whitelist" additionally reveals a Max Amount input field |
| R04-11 | Decline action closes the dialog directly and returns "declined" |

### 5.2 Test Cases

#### TC-M04-01 Complete Authorization Flow (No Active Key + Built-in Signing)

```
Prerequisite: No active session key; received payment-auth-request

Steps:
  1. Dashboard shows amber "Payment authorization requested" banner
  2. Click "Authorize Payment"
  3. Verify: Challenge dialog displays:
     - Merchant Card: merchant DID (truncated)
     - Amount: large-font SOL amount
     - Reason: payment description
     - List Action: "This time only" selected by default
     - Slide to Authorize slider
     - Decline & Block button
  4. Verify: Loading spinner appears at the top (checking for existing keys)
  5. Verify: After spinner disappears, "No existing session key" status is displayed
  6. Drag the "Slide to Authorize" slider to the right past 85%
  7. Verify: Signing Method selector bottom sheet pops up
  8. Verify: Three options are visible — Built-in Key, Phantom/Solflare, Mobile Wallet
  9. Select "Built-in Key"
  10. Verify: ResultBanner displays "Registering session key on-chain..."
  11. Wait for registration to complete
  12. Verify: ResultBanner displays "Authorized with session key"
  13. Verify: Dialog closes after 1.2 seconds, returning "authorized"

Expected Result: Payment is successfully authorized, session key is registered on-chain
```

#### TC-M04-02 Authorization Flow (With Active Key)

```
Prerequisite: An active session key exists (registered via Session Keys page)

Steps:
  1. Received payment-auth-request
  2. Click "Authorize Payment"
  3. Verify: Challenge dialog displays, "Using existing session key" banner
  4. Drag slider to authorize
  5. Verify: Signing method selector does not pop up; existing key is used directly
  6. Verify: Displays "Authorized with existing session key"

Expected Result: Skips key creation and uses the existing active key directly
```

#### TC-M04-03 Slider Below Threshold Snaps Back

```
Prerequisite: Challenge dialog is open

Steps:
  1. Drag the "Slide to Authorize" slider to approximately 50% position
  2. Release

Expected Result: Slider snaps back to the starting position; no action is triggered
```

#### TC-M04-04 Decline Payment

```
Prerequisite: Challenge dialog is open

Steps:
  1. Click the "Decline & Block" button

Expected Result: Dialog closes, returning "declined"
```

#### TC-M04-05 List Action — Add Whitelist

```
Prerequisite: Challenge dialog is open

Steps:
  1. Click the "Whitelist" chip
  2. Verify: Chip is highlighted (green border)
  3. Verify: Label input field appears below
  4. Verify: Max Amount input field appears below
  5. Enter Label: "ShopX Marketplace"
  6. Enter Max Amount: "1000000000"
  7. Slide to authorize

Expected Result: Authorization request includes list_action="add_whitelist", label="ShopX Marketplace", max_amount=1000000000
```

#### TC-M04-06 List Action — Add Blacklist

```
Prerequisite: Challenge dialog is open

Steps:
  1. Click the "Blacklist" chip
  2. Verify: Chip is highlighted (red border)
  3. Verify: Label input field appears; no Max Amount input field
  4. Enter Label: "Scam Site"
  5. Slide to authorize

Expected Result: Authorization request includes list_action="add_blacklist", label="Scam Site"
```

#### TC-M04-07 List Action — Remove Operations

```
Prerequisite: Challenge dialog is open

Steps:
  1. Click "Remove WL" chip -> Verify highlight
  2. Verify: No additional input fields
  3. Switch to "Remove BL" -> Verify highlight
  4. Slide to authorize

Expected Result: list_action is "remove_whitelist" / "remove_blacklist" respectively
```

#### TC-M04-08 Signing Method — Deep Link

```
Prerequisite: Challenge dialog is open; Phantom wallet installed

Steps:
  1. Drag slider past 85%
  2. In the signing method selector, choose "Phantom / Solflare"
  3. Verify: Displays "Open wallet to sign transaction..." ResultBanner
  4. Verify: Attempts to open Phantom wallet (redirects if installed)

Expected Result: Generates an unsigned tx and stores it as pending, constructs a Phantom deep link URL
```

#### TC-M04-09 Network Error During Authorization

```
Prerequisite: Network disconnected

Steps:
  1. Trigger the Challenge dialog
  2. Select Built-in Key signing
  3. Wait for timeout

Expected Result: ResultBanner displays a red "Error: ..." message; slider returns to an operable state
```

### 5.3 Interaction Flow Diagram

```
payment-auth-request arrives
         |
         v
Dashboard displays amber banner
         |
   "Authorize Payment"
         |
         v
ChallengeScreen opens
    |
    +-- Check active keys (loading spinner)
    |       |
    |   +--Has active key-----------+
    |   |                            |
    |   v                            v
    | "Using existing           Signing method selector
    |  session key"            +----+---------+
    |       |              Built-in  Deep Link  MWA
    |       |                  |         |        |
    |       |                  v         v        v
    |       |           createWith    build     (stub ->
    |       |           BuiltInKey    Unsigned  Built-in)
    |       |                  |      Tx
    |       |                  |         |
    |       |                  v         v
    |       |           on-chain    Open wallet app
    |       |           register    (await callback)
    |       |                  |         |
    |       |                  v         v
    |       +----------------> sendAuthResponse
    |                    withSessionKey
    |                         |
    |                    +--Success--+--Error--> ResultBanner red
    |                    |
    |                    v
    |              "Authorized" banner
    |              Pop('authorized') after 1.2s
    |
    +-- "Decline & Block" -> pop('declined')
```

---

## 6. M05 — Session Key Management

### 6.1 Business Rules

| Rule ID | Constraint |
|:--------|:-----------|
| R05-01 | Session keys are registered on-chain via the Solana Session Program (Program ID: `6EFvVTh7...`) |
| R05-02 | Local storage format: sled key `session:{base58_pubkey}` -> `[64B keypair \| 8B expires_at LE \| 8B spending_limit LE]` |
| R05-03 | Key status determination: `expires_at < now` -> "expired", otherwise -> "active" |
| R05-04 | Revoke operation executes on-chain (submits revoke_session instruction); Delete operation only removes the local record |
| R05-05 | Registration via Session Keys page defaults: spending_limit=5 SOL, duration=86400s (24h) |
| R05-06 | Registration via Challenge authorization defaults: spending_limit=payment amount x 10, duration=3600s (1h) |
| R05-07 | Local record is not deleted before Revocation completes (still visible but marked as expired) |
| R05-08 | Delete requires a confirmation dialog |

### 6.2 Test Cases

#### TC-M05-01 View Empty Key List

```
Prerequisite: Clear app data or no registered keys

Steps:
  1. Settings -> "Session Keys"
  2. Verify: Empty state is displayed — Key icon + "No session keys registered" + "Register a new key..." description
  3. Verify: "Register New Key" gradient button is displayed at the top

Expected Result: Empty state is correctly displayed
```

#### TC-M05-02 Register New Key (Built-in Method)

```
Prerequisite: Navigate to Session Keys page; network reachable

Steps:
  1. Click the "Register New Key" button
  2. Verify: Button changes to "Registering..." with a loading spinner
  3. Wait for registration to complete
  4. Verify:
     - Button reverts to "Register New Key"
     - A new key card appears in the list
     - Card displays: shortened pubkey, "active" green badge, expiration "23h 59m left", limit "5 SOL"
     - "Revoke" and "Delete" action buttons are visible

Expected Result: Key registration successful, list displays correctly
```

#### TC-M05-03 Revoke Key (On-chain Revoke)

```
Prerequisite: At least one active key exists

Steps:
  1. Click the "Revoke" button on an active key card
  2. Wait for on-chain transaction confirmation
  3. Verify: Green SnackBar "Revoked on-chain: <tx_sig>..."
  4. Verify: List updates after refresh

Expected Result: On-chain revocation successful; local record is retained (can be manually deleted)
```

#### TC-M05-04 Delete Local Key — Confirm

```
Prerequisite: At least one key exists

Steps:
  1. Click the "Delete" button
  2. Verify: Confirmation dialog "Delete Local Key?" pops up
  3. Verify: Dialog explanation "This removes the key from local storage only..."
  4. Click "Delete"

Expected Result: Key is removed from the list
```

#### TC-M05-05 Delete Local Key — Cancel

```
Prerequisite: At least one key exists

Steps:
  1. Click the "Delete" button
  2. Confirmation dialog pops up
  3. Click "Cancel"

Expected Result: Key remains in the list; no changes
```

#### TC-M05-06 Key Expired Status

```
Prerequisite: Wait for a key to pass its expires_at time

Steps:
  1. Open the Session Keys page
  2. View the expired key card

Expected Result:
  - Status badge displays "expired" (red)
  - Expiration time displays "Expired"
  - Revoke and Delete actions remain available
```

#### TC-M05-07 Deep Link Callback Completes Registration

```
Prerequisite: Selected Deep Link signing method via Challenge screen; external wallet has signed

Steps:
  1. External wallet signs and calls back ignitepay://onchain?signature=<sig>
  2. Verify: When app returns to foreground, MainNavigator captures the deep link
  3. Verify: Calls SessionKeyService.completeRegistration(signature)
  4. Verify: New key appears in the Session Keys list
  5. Verify: debugPrint outputs "Session key registered: <pubkey>"

Expected Result: Deep Link callback correctly completes key registration
```

#### TC-M05-08 Registration Failure Handling

```
Prerequisite: Network unreachable

Steps:
  1. Click "Register New Key"
  2. Wait for timeout

Expected Result: Red SnackBar "Registration failed: ..." is displayed
```

### 6.3 Interaction Flow Diagram

```
Settings -> Session Keys
        |
        +-- Register New Key
        |       |
        |   createWithBuiltInKey(5 SOL, 24h)
        |       |
        |   +--Success--+--Error--> Red SnackBar
        |   |
        |   v
        | Green SnackBar + list refresh
        |
        +-- Revoke (a key)
        |       |
        |   revoke_session_key_onchain()
        |       |
        |   +--Success--+--Error--> Red SnackBar
        |   |
        |   v
        | Green SnackBar "Revoked on-chain: ..."
        |
        +-- Delete (a key)
                |
            Confirmation dialog
              |       |
           Cancel    Delete
              |       |
            No change  delete_session_key_local()
                      |
                      v
                  Key removed from list
```

---

## 7. M06 — Messaging

### 7.1 Business Rules

| Rule ID | Constraint |
|:--------|:-----------|
| R06-01 | Message list is sorted in reverse chronological order (newest first) |
| R06-02 | Filters support: All, Payment, List Sync, Connection |
| R06-03 | Clicking a Payment type message -> Opens ChallengeScreen |
| R06-04 | Clicking a non-Payment type message -> Opens Message Detail Dialog |
| R06-05 | Pull-to-refresh triggers Mediator reconnection + message fetch |
| R06-06 | Empty list displays "No messages yet" + "Check for messages" button |

### 7.2 Test Cases

#### TC-M06-01 View Message List

```
Prerequisite: Mediator connected; historical messages exist

Steps:
  1. Switch to the Messages tab
  2. Verify: Message list is displayed
  3. Verify: Each message card contains: type icon, merchant DID (truncated), description, amount (for Payment type)

Expected Result: Messages are correctly displayed in reverse chronological order
```

#### TC-M06-02 Filter Messages

```
Prerequisite: Message list contains multiple message types

Steps:
  1. Click the "Payment" filter chip
  2. Verify: Only payment type messages are displayed
  3. Click the "List Sync" chip
  4. Verify: Only list-sync type messages are displayed
  5. Click "All"
  6. Verify: All messages are displayed

Expected Result: Filtering works correctly
```

#### TC-M06-03 Click Payment Message

```
Prerequisite: Message list contains Payment type messages

Steps:
  1. Click a Payment message

Expected Result: Opens ChallengeScreen with the message's paymentId, merchantDid, amount, description
```

#### TC-M06-04 Click Non-Payment Message

```
Prerequisite: Message list contains non-Payment type messages (e.g., list-sync-notification)

Steps:
  1. Click the message

Expected Result: Message Detail Dialog pops up, displaying all fields (msgType, rawBody, etc.)
```

#### TC-M06-05 Pull to Refresh

```
Prerequisite: Messages page is open

Steps:
  1. Pull down the page
  2. Verify: RefreshIndicator is displayed
  3. Wait for refresh to complete

Expected Result: Reconnects to Mediator and fetches the latest messages
```

#### TC-M06-06 Empty Message List

```
Prerequisite: Mediator connected but no messages

Steps:
  1. Switch to the Messages tab
  2. Verify: Empty state is displayed — inbox icon + "No messages yet"
  3. Click the "Check for messages" button
  4. Verify: Message fetch is triggered

Expected Result: Empty state is correctly displayed; manual fetch works properly
```

---

## 8. M07 — Risk Control Policies

### 7.1 Business Rules

| Rule ID | Constraint |
|:--------|:-----------|
| R07-01 | Policy cards can be expanded/collapsed |
| R07-02 | Auto-pay toggle can be switched |
| R07-03 | Limit input supports SOL/USD unit switching |
| R07-04 | Validity period is set via date picker |
| R07-05 | Current data is mock/hardcoded (ShopX, DeFi, NFT, RPC) |

### 8.2 Test Cases

#### TC-M07-01 View Policy List

```
Steps:
  1. Dashboard -> "Policies" or Settings -> "Policy Architect"
  2. Verify: 4 merchant policy cards are displayed (ShopX, DeFi, NFT, RPC)
  3. Verify: Statistics grid shows Merchants=4, Auto-Pay=2, Weekly Cap=3.00 SOL, Spent=0.47 SOL

Expected Result: Policy page displays correctly
```

#### TC-M07-02 Expand Policy Details

```
Steps:
  1. Click a merchant policy card
  2. Verify: Card expands to show details:
     - Auto-pay toggle
     - Single Limit input field + SOL/USD switch
     - Weekly Velocity progress bar
     - Expiry date picker + days badge

Expected Result: Card correctly expands with all sub-components visible
```

#### TC-M07-03 Toggle Auto-pay

```
Steps:
  1. Expand a merchant policy
  2. Toggle the Auto-pay switch

Expected Result: Switch state toggles (currently a UI demo, does not affect actual logic)
```

---

## 9. M08 — App Settings

### 9.1 Business Rules

| Rule ID | Constraint |
|:--------|:-----------|
| R08-01 | Switching Network automatically updates the RPC URL (devnet -> `https://api.devnet.solana.com`, mainnet-beta -> `https://api.mainnet-beta.solana.com`) |
| R08-02 | Program IDs are read-only display (State Channel, DID ZK, Session Key) |
| R08-03 | Payment Mode switching: Self-Funded / Sponsored |
| R08-04 | Clear Cache requires confirmation dialog |

### 9.2 Test Cases

#### TC-M08-01 Network Switching

```
Steps:
  1. Settings -> Solana Network
  2. Select "mainnet-beta"
  3. Verify: RPC URL automatically updates to https://api.mainnet-beta.solana.com
  4. Select "devnet"
  5. Verify: RPC URL reverts to https://api.devnet.solana.com

Expected Result: Network switching correctly updates the RPC URL
```

#### TC-M08-02 Custom RPC URL

```
Steps:
  1. Enter a custom URL in the RPC URL input field
  2. Navigate to another page and return

Expected Result: Custom URL is persisted
```

#### TC-M08-03 View Program IDs

```
Steps:
  1. Scroll to the Program IDs section
  2. Verify three read-only IDs are displayed:
     - State Channel: DJBHr35j...
     - DID ZK Compression: ignDID...
     - Session Key: 6EFvVTh7...

Expected Result: Program IDs are correctly displayed and not editable
```

#### TC-M08-04 Payment Mode Switching

```
Steps:
  1. In the Payment Mode section, switch to "Sponsored"
  2. Verify: Selected state updates
  3. Restart the app
  4. Verify: Setting is persisted

Expected Result: Payment mode switches and is persisted
```

#### TC-M08-05 Clear Cache

```
Steps:
  1. Click "Clear Cache"
  2. Verify: Confirmation dialog pops up
  3. Click confirm

Expected Result: Operation result SnackBar is displayed
```

---

## 10. M09 — Push Channel

### 10.1 Business Rules

| Rule ID | Constraint |
|:--------|:-----------|
| R09-01 | Overseas users (non-zh_CN locale) use FCM push |
| R09-02 | Mainland China users (zh_CN / Hans) use WebSocket persistent connection |
| R09-03 | FCM foreground notification displays local notification (title "Payment Authorization") |
| R09-04 | FCM background is handled via top-level handler |
| R09-05 | WS auto-reconnects after disconnection (3-second delay) + fetches messages missed during disconnection |
| R09-06 | Any channel restoration triggers HTTPS Pull fallback fetch |

### 10.2 Test Cases

#### TC-M09-01 FCM Push Reception (Overseas Users)

```
Prerequisite: Overseas locale (en-US); FCM token registered; app in foreground

Steps:
  1. MCP Server sends a payment request
  2. Mediator pushes signal via FCM
  3. Verify: Device receives local notification "Payment Authorization" / "New payment request received"
  4. Verify: DidcommService triggers message fetch
  5. Verify: Dashboard displays pending auth banner

Expected Result: FCM push correctly triggers message fetch and UI update
```

#### TC-M09-02 WebSocket Push Reception (Chinese Users)

```
Prerequisite: Chinese locale (zh_CN); Mediator connected

Steps:
  1. MCP Server sends a payment request
  2. Mediator pushes JWE directly via WS
  3. Verify: _onWsMessage triggers
  4. Verify: Message is decrypted and added to messages list
  5. Verify: Dashboard displays pending auth banner

Expected Result: WS push is correctly received and processed
```

#### TC-M09-03 WS Disconnect and Reconnect

```
Prerequisite: Chinese user; WS connection normal

Steps:
  1. Disconnect network
  2. Verify: WS connection drops, triggering onDone callback
  3. Restore network
  4. Wait 3 seconds for reconnect
  5. Verify: WS re-establishes connection
  6. Verify: Fetches messages missed during disconnection

Expected Result: Auto-reconnects and retrieves missed messages
```

---

## 11. M10 — Deep Link Callback

### 11.1 Business Rules

| Rule ID | Constraint |
|:--------|:-----------|
| R10-01 | AndroidManifest registers the `ignitepay://` scheme |
| R10-02 | Callback path is `ignitepay://onchain?signature=<base58>` |
| R10-03 | MainNavigator listens for callbacks via `app_links` |
| R10-04 | Callback triggers `SessionKeyService.completeRegistration(signature)` |
| R10-05 | After successful callback, the pending unsigned tx is cleared |

### 11.2 Test Cases

#### TC-M10-01 Normal Callback Flow

```
Prerequisite: SessionKeyService has a pending unsigned tx

Steps:
  1. External wallet completes signing
  2. Wallet opens callback URL: ignitepay://onchain?signature=<valid_base58_signature>
  3. Verify: App receives the deep link
  4. Verify: _handleDeepLink parses the signature parameter
  5. Verify: Calls completeRegistration(signature)
  6. Verify: pendingUnsignedTx is cleared
  7. Verify: debugPrint "Session key registered: <pubkey>"

Expected Result: Deep Link callback correctly completes registration
```

#### TC-M10-02 Callback Received With No Pending Transaction

```
Prerequisite: SessionKeyService has no pending unsigned tx

Steps:
  1. Open ignitepay://onchain?signature=xxx

Expected Result: completeRegistration throws exception "No pending unsigned transaction", caught by catchError and logged
```

#### TC-M10-03 Invalid Signature Callback

```
Prerequisite: SessionKeyService has a pending unsigned tx

Steps:
  1. Open ignitepay://onchain?signature=invalid_signature
  2. completeRegistration attempts to use the invalid signature

Expected Result: Rust layer returns an error (signature verification failure or RPC submission failure), caught by catchError
```

---

## 12. Cross-Module End-to-End Flows

### E2E-01 Complete Payment Authorization End-to-End

```
1. First launch -> Complete wizard (TC-M01-01)
2. Configure Mediator connection (TC-M03-03)
3. Scan QR code to pair MCP (TC-M03-01)
4. MCP sends payment-auth-request -> Dashboard displays banner
5. Click "Authorize Payment" (TC-M04-01)
6. Select Built-in Key signing -> Wait for on-chain registration
7. Authorization successful -> Dialog closes
8. Open Session Keys -> Verify new key exists (TC-M05-02)
9. MCP sends another payment request -> Reuse existing key during authorization (TC-M04-02)
```

### E2E-02 Deep Link End-to-End

```
1. Pair MCP and trigger a payment request
2. In Challenge, select "Phantom/Solflare" signing
3. Verify: Pending unsigned tx is created
4. External wallet signs and calls back (TC-M10-01)
5. Verify: New key appears in Session Keys list
6. Subsequent payments reuse this key
```

### E2E-03 Key Lifecycle

```
1. Register key (TC-M05-02) -> Status "active"
2. Wait for expiration -> Status changes to "expired" (TC-M05-06)
3. Register new key -> "active" again
4. Revoke old key (TC-M05-03) -> On-chain revoke
5. Delete old local key record (TC-M05-04) -> Removed from list
```

---

## 13. Data Validation Rules Summary

| Field | Validation Rule | Error Message |
|:------|:---------------|:--------------|
| DID | Format `did:ignite:z6Mk...`, length >= 32 characters | Internal error, not user input |
| WS URL | Valid WebSocket URL (`ws://` or `wss://`) | "Failed to connect to mediator" |
| QR Content | Must start with `didcomm://` | "Invalid invitation URL" |
| Spending Limit | Positive integer (lamports) | Rust layer parameter validation |
| Duration | Positive integer (seconds) | Rust layer parameter validation |
| Label | Non-empty string (when list action is add) | Not validated, can be empty |
| Max Amount | Positive integer (lamports), optional | int.tryParse -> null |
| Owner Signature | Base58-encoded 64-byte Ed25519 signature | "Invalid owner signature length" |
| Session Pubkey | Base58-encoded 32-byte Ed25519 public key | sled lookup failure |

---

## 14. Boundary Conditions & Exception Scenarios

| Scenario | Expected Behavior |
|:---------|:------------------|
| Authorize while network is disconnected | ResultBanner displays "Error: ..." + specific error message |
| Incorrect Mediator address | Connection timeout, retry or skip available |
| Scanning the same MCP QR code twice | Second scan behaves the same as the first (no duplicate prevention) |
| Payment request with amount of 0 | Amount displays "0 SOL" |
| Very large amount (>10,000 SOL) | Amount displays normally (not truncated) |
| Receiving multiple payment requests simultaneously | The latest one is taken as pendingAuth |
| App receives FCM while in background | Triggers message fetch; UI updates when returning to foreground |
| Multiple keys expiring simultaneously | All marked "expired" in the list |
| Revoking an already expired key | Still sends on-chain transaction (not blocked) |
| Deleting an active key currently in use | Deletes local record (does not check if in use) |
| Deep Link callback when app is not running | After cold start, app_links may not trigger; manual action required |

---

## 15. Test Matrix

### 15.1 Platform Coverage

| Platform | Version | Architecture | Test Requirement |
|:---------|:--------|:-------------|:-----------------|
| Android Emulator | API 36 | x86_64 | Full test suite |
| Android Physical | API 34+ | arm64-v8a | M05, M10 require physical device |
| iOS | 17+ | arm64 | Not adapted (not tested) |

### 15.2 Network Environment

| Environment | Covered Modules |
|:------------|:----------------|
| Normal network (Devnet reachable) | Full suite |
| Weak network (high latency) | M04, M05 timeout behavior |
| No network | M03, M04, M05 error handling |
| VPN/Proxy | M09 FCM reachability |

### 15.3 User Region

| Locale | Push Channel | Test Focus |
|:-------|:-------------|:-----------|
| en-US | FCM | FCM registration, foreground notifications |
| zh_CN | WebSocket | WS persistent connection, disconnect reconnect |
| Other | FCM | Fallback to FCM |
