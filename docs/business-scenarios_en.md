# Ignite Pay Business Scenario Operations Manual

This document is organized by business events and describes the operational steps for all core business processes in the Ignite Pay system. Each process contains multiple use cases, and each use case includes prerequisites, participating roles, detailed steps, expected results, and exception handling.

---

## Table of Contents

1. [User DID Identity Creation](#business-event-1-user-did-identity-creation)
2. [App Establishes Connection with MCP Server](#business-event-2-app-establishes-connection-with-mcp-server)
3. [DIDComm Mediator Authentication](#business-event-3-didcomm-mediator-authentication)
4. [X402 Payment Authorization](#business-event-4-x402-payment-authorization)
5. [Session Key On-Chain Payment](#business-event-5-session-key-on-chain-payment)
<!-- State Channel: Exploration phase, not enabled
6. [State Channel Opening](#business-event-6-state-channel-opening)
7. [State Channel Off-Chain Payment](#business-event-7-state-channel-off-chain-payment)
-->
8. [QR Code Payment Collection](#business-event-8-qr-code-payment-collection)
<!-- State Channel: Exploration phase, not enabled
9. [State Channel Closure and Settlement](#business-event-9-state-channel-closure-and-settlement)
10. [Hub Registration and Discovery](#business-event-10-hub-registration-and-discovery)
11. [Multi-Hop Routing Payment](#business-event-11-multi-hop-routing-payment)
-->
12. [Merchant DID Onboarding](#business-event-12-merchant-did-onboarding)
13. [Message Push Notifications](#business-event-13-message-push-notifications)
14. [Merchant DID Lifecycle Management](#business-event-14-merchant-did-lifecycle-management)
<!-- State Channel: Exploration phase, not enabled
15. [State Channel Operations](#business-event-15-state-channel-operations)
16. [Hub Network Topology Management](#business-event-16-hub-network-topology-management)
-->
17. [App-Side Management and Settings](#business-event-17-app-side-management-and-settings)
<!-- State Channel: Exploration phase, not enabled
18. [Compliance and Risk Control](#business-event-18-compliance-and-risk-control)
-->

> The titles in the table of contents correspond one-to-one with the section titles in the body. The colons (`:`) in the titles reflect the actual format in the body. GitHub and most Markdown renderers automatically convert titles to lowercase and replace spaces with `-` to generate anchors.

---

## Business Event 1: User DID Identity Creation

### Use Case 1.1: User App First Launch -> Automatic DID Generation

**Prerequisites**:
- Sentinel App installed on mobile phone
- First launch, no existing identity data locally

**Participating Roles**: User, Sentinel App

**Detailed Steps**:

1. User opens Sentinel App
2. App detects no DID identity records in the local sled database
3. Enter OnboardingScreen three-step guided flow:
   - Step 1: Welcome page introducing App features
   - Step 2: Click "Generate Identity" button
4. App calls Rust bridge `initialize_identity()`:
   - Generate Ed25519 signing key pair
   - Derive X25519 key agreement key from public key
   - Build DID identifier: `did:ignite:z<multibase-base58btc>`
   - Encoded content: `0xed 0x01` (multicodec Ed25519 prefix) + 32 bytes Ed25519 public key
5. Build W3C DID Document:
   - `verificationMethod`: Ed25519VerificationKey2020 (`#key-signing-1`)
   - `keyAgreement`: X25519KeyAgreementKey2020 (`#key-agreement-1`)
   - `service`: IgnitePolicyList endpoint (initial CID is empty)
6. Persist key pair and DID Document to sled local database
7. Step 3: Configure Mediator connection (skippable)
8. Enter Dashboard main page

**Expected Results**:
- User has a `did:ignite:z...` decentralized identity
- DID Document contains signing key and encryption key
- Identity data securely stored in local sled database

**Exception Handling**:
- sled database write failure -> display error prompt, retry generation
- Key pair generation failure (system random source unavailable) -> prompt system error

---

### Use Case 1.2: Merchant App First Launch -> Automatic DID Generation

**Prerequisites**:
- Ignite Merchant App installed
- First launch

**Participating Roles**: Merchant, Merchant App

**Detailed Steps**:

1. Merchant opens Ignite Merchant App
2. Enter OnboardingScreen:
   - Fill in Hub Endpoint URL (e.g. `http://hub.example.com:3003`)
   - Fill in Mediator WebSocket URL (optional, e.g. `ws://mediator.example.com:8080/ws`)
3. App calls Rust bridge `initialize_merchant()`:
   - Generate Ed25519 key pair
   - <!-- State Channel: Exploration phase, not enabled - originally "Generate state channel DID" -->Generate merchant DID: `did:ignite:<raw_base58>`
   - Store in sled `keypairs` tree
4. App calls Rust bridge `initialize_merchant_comm()`:
   - Generate independent Ed25519 + X25519 key pair
   - Generate DIDComm communication DID: `did:ignite:z<multicodec_base58>`
   - Store in sled `didcomm_identity` tree
5. Connect to Mediator (if configured)
6. Enter Merchant Dashboard

**Expected Results**:
- Merchant has two independent DIDs:
  - State channel DID (for QR codes, channel operations, on-chain signing)
  - DIDComm communication DID (for JWE encryption/decryption, Mediator messaging)
- Two key systems are completely independent

**Exception Handling**:
- Hub Endpoint unreachable -> display warning, allow continuing (configure later)
- Mediator connection failure -> display offline status, allow reconnecting later

---

### Use Case 1.3: MCP Server First Launch -> Automatic DID Generation

**Prerequisites**:
- MCP Server binary compiled
- config.toml configured

**Participating Roles**: System Administrator, MCP Server

**Detailed Steps**:

1. Administrator starts MCP Server: `cargo run -p ignite-pay-mcp`
2. Server checks sled database (`./data`) for existing DID identity
3. If no existing identity:
   - Automatically generate Ed25519 + X25519 key pair
   - Derive `did:ignite` identifier
   - Persist to sled database
4. If identity exists:
   - Automatically load existing key pair and DID
5. Connect to Mediator WebSocket (`ws://127.0.0.1:8080/ws`)
6. Execute DIDComm Mediator handshake (-> Use Case 2.3)
7. Enter message receiving loop

**Expected Results**:
- MCP Server has a `did:ignite` identity
- Connected to Mediator and handshake completed
- Can receive/send DIDComm encrypted messages

**Exception Handling**:
- Mediator unreachable -> auto-reconnect every 3 seconds
- sled database corrupted -> restore from backup or regenerate identity

---

## Business Event 2: App Establishes Connection with MCP Server

### Use Case 2.1: User App Scans QR Code to Pair with User MCP

**Prerequisites**:
- User has generated DID identity (Use Case 1.1)
- MCP Server has started and connected to Mediator (Use Case 1.3)
- MCP Server has generated OOB invitation QR code

**Participating Roles**: User, Sentinel App, MCP Server, Mediator

**Detailed Steps**:

1. MCP Server generates OOB invitation QR code, format: `didcomm://?_oob=<base64>`
2. User clicks "Scan MCP QR Code" on Sentinel Dashboard
3. Opens full-screen QR scanner (QrScannerScreen)
4. Scans MCP Server's QR code
5. App calls Rust bridge `parse_oob_invitation()` to parse invitation:
   - Extract MCP Server's DID
   - Extract Mediator WebSocket endpoint
6. App calls `connect_mediator()` to connect to Mediator WebSocket
7. App calls `send_connection_request()` to send `connection-request` DIDComm message:
   - Message relayed through Mediator to MCP Server
   - Contains user's DID Document
8. MCP Server receives `connection-request`, registers user as communication peer
9. MCP Server returns `connection-response`, containing its own DID Document
10. App receives response, registers MCP Server as communication peer
11. Pairing complete, MCP Server appears in connection management list (ConnectionScreen)

**Expected Results**:
- User App and MCP Server have established a DIDComm P2P encrypted connection
- Both parties hold each other's public keys and can communicate end-to-end encrypted
- MCP Server appears in App's connection management list

**Exception Handling**:
- Invalid QR code format -> prompt "Invalid QR code"
- Mediator connection failure -> prompt network error
- MCP Server unresponsive -> prompt retry after timeout

---

### Use Case 2.2: Merchant App Scans QR Code to Pair with Merchant MCP

**Prerequisites**:
- Merchant has generated dual DID identity (Use Case 1.2)
- Merchant MCP Server has started and connected to merchant-side Mediator (`:4000`)

**Participating Roles**: Merchant, Merchant App, Merchant MCP, Mediator

**Detailed Steps**:

1. Merchant MCP Server generates OOB invitation QR code
2. Merchant scans QR code in Merchant App
3. App calls `parse_oob_invitation()` to parse invitation, extract MCP Server's DID
4. App connects to merchant-side Mediator WebSocket
5. **Key difference**: Merchant App uses **DIDComm communication DID** (`did:ignite:z...`, stored in `didcomm_identity` tree) for the following:
   - JWE encryption: Use communication DID's X25519 key (`#key-agreement-1`) to encrypt messages
   - DIDComm signing: Use communication DID's Ed25519 key (`#key-signing-1`) to sign
   - Does **not** use state channel DID (`did:ignite:<raw_base58>`, stored in `keypairs` tree)
6. App sends `connection-request` (JWE encrypted), containing communication DID's DID Document
7. Merchant MCP registers merchant App's communication DID as peer
8. Merchant MCP returns `connection-response`
9. Pairing complete, Merchant MCP appears in merchant App's connection management list

**Expected Results**:
- Merchant App and Merchant MCP have established a DIDComm connection
- Both parties use their respective DIDComm communication DIDs for encrypted communication
- State channel DID remains independent, used only for QR code generation and on-chain signing

**Exception Handling**:
- Invalid QR code format -> prompt "Invalid QR code"
- Mediator connection failure -> prompt network error
- MCP Server unresponsive -> prompt retry after timeout

---

### Use Case 2.3: MCP Server Connects to DIDComm Mediator (WebSocket Handshake + Authentication)

**Prerequisites**:
- MCP Server has generated DID identity
- Mediator service running on `:8080`

**Participating Roles**: MCP Server, Mediator

**Detailed Steps**:

1. MCP Server connects to `ws://127.0.0.1:8080/ws` via WebSocket
2. Execute three-step plaintext handshake:

| Step | Direction | Message Type | Description |
|:-----|:-----|:---------|:-----|
| 1 | Client -> Mediator | `coordinate-mediation/2.0/mediate-request` | Register as Mediator client |
| 2 | Mediator -> Client | `coordinate-mediation/2.0/mediate-grant` | Mediator confirms |
| 3 | Client -> Mediator | `coordinate-mediation/2.0/keylist-update` (add) | Register receiving key `{did}#key-1` |
| 4 | Mediator -> Client | `coordinate-mediation/2.0/keylist-update-response` | Confirm key registration |
| 5 | Client -> Mediator | `peer-did-discovery/1.0/discover` | Send complete DID Document |

3. Enter encrypted message receiving loop after handshake completion
4. All subsequent messages transmitted via JWE authcrypt encryption

**Expected Results**:
- MCP Server registered with Mediator
- Mediator can route messages to MCP Server based on DID
- MCP Server can receive encrypted DIDComm messages

**Exception Handling**:
- WebSocket connection failure -> auto-reconnect every 3 seconds
- Handshake timeout -> close connection and reconnect

---

### Use Case 2.4: MCP Server Reconnects to Mediator After Disconnection

**Prerequisites**:
- MCP Server has completed initial handshake (Use Case 2.3)
- WebSocket connection unexpectedly disconnected

**Participating Roles**: MCP Server, Mediator

**Detailed Steps**:

1. Detect WebSocket connection disconnect
2. Wait 3 seconds
3. Re-establish WebSocket connection
4. Re-execute full handshake (mediate-request -> mediate-grant -> keylist-update -> peer-did-discovery)
5. After successful handshake, pull offline queued messages via Message Pickup 3.0 protocol:
   - Send `messagepickup/3.0/status-request`
   - Receive `status` (returns count of queued messages)
   - Send `messagepickup/3.0/batch-pickup` for batch retrieval
   - Receive `batch` returning batch messages
6. Resume normal encrypted message receiving loop

**Expected Results**:
- MCP Server reconnected to Mediator
- All messages from offline period have been retrieved
- Message processing resumed

**Exception Handling**:
- Reconnection failure -> continue retrying every 3 seconds
- Offline messages older than 7 days supplemented via `GET /v1/sync/list`

---

## Business Event 3: DIDComm Mediator Authentication

### Use Case 3.1: User App Authenticates Mediator via Challenge-Response

**Prerequisites**:
- User App has generated DID identity
- Mediator service running on `:8080`

**Participating Roles**: Sentinel App, Mediator

**Detailed Steps**:

1. App calls `GET /v1/auth/challenge` to obtain authentication nonce
2. App signs nonce using DID Ed25519 private key
3. App calls `POST /v1/auth/token` to send signature in exchange for JWT:
   - Request Body: `{ "did": "did:ignite:z...", "signature": "<base64>" }`
4. Mediator verifies signature validity
5. Mediator issues JWT (containing `user_did` field)
6. App receives Bearer Token, subsequent API calls include this Token

**Expected Results**:
- App receives JWT Bearer Token
- All subsequent Mediator API calls include `Authorization: Bearer <token>` header

**Exception Handling**:
- Signature verification failure -> returns 401, App re-initiates authentication
- Token expired -> App automatically re-executes Challenge-Response

---

### Use Case 3.2: Merchant App Authenticates Mediator via Challenge-Response

**Prerequisites**: Same as Use Case 3.1

**Participating Roles**: Merchant App, Mediator

**Detailed Steps**:
- Same as Use Case 3.1, but uses merchant's **DIDComm communication DID** (not state channel DID) for signing

**Expected Results**: Same as Use Case 3.1

**Exception Handling**: Same as Use Case 3.1

---

### Use Case 3.3: MCP Server Authenticates Mediator via DID Signature

**Prerequisites**:
- MCP Server has generated DID identity

**Participating Roles**: MCP Server, Mediator

**Detailed Steps**:

1. MCP Server automatically completes authentication during WebSocket handshake
2. Signs Mediator challenge using DIDComm Agent's Ed25519 key
3. After Mediator verifies signature, binds WebSocket connection to DID

**Expected Results**:
- WebSocket connection bound to MCP Server DID
- Mediator can route messages to this connection based on DID

**Exception Handling**:
- Signature verification failure -> Mediator closes WebSocket connection

---

## Business Event 4: X402 Payment Authorization

### Use Case 4.1: AI Agent Initiates Payment -> MCP Auto-Approves (Whitelist/Low Amount)

**Prerequisites**:
- AI Agent connected to User MCP Server via MCP protocol
- MCP Server configured with `[policy] auto_approve_max` or user has added merchant to whitelist
- Merchant registered on-chain and holds valid VC

**Participating Roles**: AI Agent, External Service Provider, MCP Server, Solana Blockchain

**Detailed Steps**:

1. AI Agent sends HTTP request to external service provider
2. Service provider returns `402 Payment Required` (X402 protocol extension)
3. Agent calls MCP Tool `process_x402_challenge`, passing `challenge_body`
4. MCP Server parses 402 response:
   - Extract from `accepts[]`: paymentType, network, token, amount, recipient
   - Extract from X402 extension headers: `x402-merchant-did`, `x402-payment-address`, `x402-merkle-context`
5. **Merchant Verification**:
   - VC signature verification: Verify Ed25519Signature2020 proof using built-in platform public key
   - On-chain Merkle Proof verification: Obtain proof via Helius DAS API, verify locally
   - Consistency check: VC DID public key hash == on-chain merchant_did_hash
6. **Decision Evaluation** (priority from high to low):
   - Merchant verification passed
   - Query sled list cache: `merchant_did` in whitelist && `amount <= list_max_amount`
   - Or `amount <= auto_approve_max && auto_approve_max > 0`
7. **Auto-approve**: Execute on-chain payment using existing Session Key (-> Use Case 5.2 or 5.3)
8. Return payment result and on-chain transaction signature to Agent

**Expected Results**:
- Payment automatically executed without user phone interaction
- Agent receives payment proof (transaction signature)
- Agent re-requests resource using payment proof

**Exception Handling**:
- VC verification failure -> reject payment, return verification failure reason
- On-chain Merkle Proof verification failure -> reject payment
- Session Key expired or insufficient quota -> fallback to interactive authorization (-> Use Case 4.2)

---

### Use Case 4.2: AI Agent Initiates Payment -> MCP Pushes to User App -> User Approves

**Prerequisites**:
- Same as Use Case 4.1
- Merchant not in whitelist or amount exceeds `auto_approve_max`
- User App and MCP Server are paired (-> Use Case 2.1)

**Participating Roles**: AI Agent, MCP Server, Mediator, User, Sentinel App, Solana Blockchain

**Detailed Steps**:

1. Agent encounters 402 -> calls MCP `process_x402_challenge`
2. MCP parses 402, executes merchant verification (passed)
3. **Decision Evaluation**: Not in whitelist and exceeds threshold -> requires interactive authorization
4. MCP creates PaymentRequest (status: PendingAuth), saves to sled
5. MCP builds `payment-auth-request` DIDComm message (JWE authcrypt encrypted):
   ```json
   { "payment_id": "uuid-v4", "merchant_did": "did:ignite:z...", "amount": 50000000, "description": "API Service Call" }
   ```
6. MCP sends to user App via Mediator:
   - Overseas users: FCM signal -> App HTTPS pull -> decrypt
   - Domestic users: WS direct push -> App decrypts directly
7. User App receives message, decrypts and pops up full-screen ChallengeScreen:
   - Display: Merchant DID, amount (SOL), payment reason
   - Policy configuration: Daily limit, per-transaction limit, validity period
   - List action selection: One-time only / Add to whitelist / Add to blacklist
   - Signing method selection: Built-in key / Phantom deep link / Solflare / MWA (Android)
8. User reviews and clicks Approve
9. App creates Session Key:
   - Calls `create_session_key_for_payment()` to generate Ed25519 ephemeral key pair
   - Build on-chain registration transaction (SessionToken PDA)
   - User signs -> submit to Solana
   - On-chain confirmation
10. App builds `payment-auth-response` JWE message:
    ```json
    {
      "payment_id": "uuid-v4",
      "authorized": true,
      "session_key_pubkey": "Base58Pubkey",
      "session_key_tx_signature": "Base58TxSig",
      "session_expires_at": 1713703600,
      "spending_limit": 100000000,
      "scopes": ["sol:transfer"],
      "list_action": "none"
    }
    ```
11. App sends back to MCP Server via Mediator
12. MCP receives authorization response:
    - Verify Session Key on-chain status
    - Build ExecutePayment transaction using Session Key
    - Submit to Solana
    - PaymentRequest status -> Executed
13. MCP returns payment result (including transaction signature) to Agent

**Expected Results**:
- User completed authorization approval
- Session Key registered on-chain
- On-chain payment executed successfully
- Agent received payment proof

**Exception Handling**:
- Mediator push failure -> MCP retries or waits for App to actively pull
- User declines -> -> Use Case 4.3
- Authorization timeout (300 seconds) -> -> Use Case 4.4
- On-chain transaction failure -> MCP returns error to Agent

---

### Use Case 4.3: AI Agent Initiates Payment -> User Declines -> List Action

**Prerequisites**: Same as Use Case 4.2

**Participating Roles**: AI Agent, MCP Server, User, Sentinel App

**Detailed Steps**:

1. User reviews payment request on ChallengeScreen
2. User selects action:
   - **Decline & Block**: Decline + add to blacklist
   - **Decline**: Decline this one only
3. App sends `payment-auth-response`:
   ```json
   {
     "payment_id": "uuid-v4",
     "authorized": false,
     "list_action": "add_blacklist",
     "list_label": "Suspicious merchant"
   }
   ```
4. MCP Server receives decline response:
   - PaymentRequest status -> Rejected
   - Parse `list_action`
5. If `list_action = "add_blacklist"`:
   - Build blacklist entry: `{ did: "merchant_did", label: "Suspicious merchant", expires: null }`
   - Append to sled local blacklist cache
   - Asynchronously upload merged list to IPFS (obtain new CID)
   - Send `list-sync-notification` to mobile (-> Use Case 13.1 or 13.2)
6. MCP returns decline information to Agent

**Expected Results**:
- Payment request declined
- If Block selected, merchant added to blacklist
- Subsequent payment requests from this merchant will be automatically blocked

**Exception Handling**:
- IPFS upload failure -> sled local cache still valid, re-sync on next startup
- List sync notification delivery failure -> App fetches latest CID from DID Document on next startup

---

### Use Case 4.4: Payment Timeout -> MCP Returns Timeout Error to Agent

**Prerequisites**:
- MCP has pushed authorization request to user App
- User has not responded within the specified time

**Participating Roles**: AI Agent, MCP Server

**Detailed Steps**:

1. MCP creates PaymentRequest and starts oneshot channel to wait
2. Wait time exceeds `auth_timeout` (default 300 seconds)
3. MCP updates PaymentRequest status to Expired
4. MCP returns timeout error to Agent:
   ```json
   { "status": "expired", "payment_id": "uuid-v4", "error": "Authorization timeout after 300s" }
   ```

**Expected Results**:
- Agent receives timeout error
- PaymentRequest status is Expired

**Exception Handling**:
- Agent can choose to retry (re-trigger 402 flow)

---

## Business Event 5: Session Key On-Chain Payment

### Use Case 5.1: Create Self-Funded Mode Session Key (self_funded)

**Prerequisites**:
- User has authorized payment on mobile (Use Case 4.2 steps 7-8)
- Solana network reachable
- User has sufficient SOL balance

**Participating Roles**: Sentinel App, Solana Blockchain

**Detailed Steps**:

1. User clicks Approve on ChallengeScreen (Use Case 4.2 step 8)
2. App calls Rust bridge `create_session_key_for_payment()`:
   - Generate Ed25519 ephemeral key pair
3. Build on-chain SessionToken PDA registration transaction:
   - `owner`: User main wallet public key
   - `ephemeral_pubkey`: Ephemeral key public key
   - `expiry`: Current time + validity period
   - `scope`: `["sol:transfer"]` or `["spl:transfer"]`
   - `spending_limit`: Authorized spending limit
4. Transaction includes `system_program::transfer`: transfer small amount of SOL from main wallet to ephemeral key address (Gas fee)
5. User selects signing method and signs
6. Submit transaction to Solana RPC
7. On-chain confirmation
8. Return `session_key_pubkey` and `chain_tx_signature`

**Expected Results**:
- Session Key registered on-chain
- Ephemeral key address has sufficient Gas fee
- Session Key can be used for subsequent on-chain payments

**Exception Handling**:
- On-chain transaction failure -> prompt user to retry
- Insufficient SOL balance -> prompt to top up
- External wallet signing failure -> prompt to change signing method

---

### Use Case 5.2: SOL Transfer Payment

**Prerequisites**:
- Session Key created and not expired (Use Case 5.1)
- Sufficient balance in spending_limit

**Participating Roles**: MCP Server, Solana Blockchain

**Detailed Steps**:

1. MCP Server receives authorization response, extracts session_key_pubkey
2. Verify Session Key on-chain status:
   - Query on-chain SessionToken PDA
   - Verify not expired: `current_slot < session_expires_at`
   - Verify `spending_limit >= current payment amount`
3. Build SOL Transfer transaction:
   - `from`: Ephemeral key address
   - `to`: Merchant receiving address
   - `amount`: Payment amount (lamports)
   - `feePayer`: Ephemeral key public key (self-funded mode)
4. Sign transaction using Session Key
5. Broadcast to Solana RPC
6. On-chain verification:
   - Session Key signature validity
   - Not expired
   - spending_limit not exceeded
   - Execute SOL transfer
   - Update Session Key spent amount
7. Return transaction signature

**Expected Results**:
- SOL transfer successful
- Session Key spending_limit deducted
- MCP receives transaction signature

**Exception Handling**:
- Session Key expired -> return error, requires re-authorization
- spending_limit insufficient -> return error
- On-chain confirmation failure -> retry or return error

---

### Use Case 5.3: SPL Token Transfer Payment

**Prerequisites**:
- Session Key created, scope includes `"spl:transfer"`
- User holds corresponding SPL Token

**Participating Roles**: MCP Server, Solana Blockchain

**Detailed Steps**:

1. Same as Use Case 5.2 steps 1-2
2. Build SPL Token Transfer transaction:
   - Use `spl_token::instruction::transfer`
   - `source`: User Token Account
   - `destination`: Merchant Token Account
   - `amount`: Token amount
3. Subsequent steps same as Use Case 5.2 steps 4-7

**Expected Results**: SPL Token transfer successful

**Exception Handling**: Same as Use Case 5.2

---

### Use Case 5.4: Session Key Expiration / Quota Exhaustion Handling

**Prerequisites**:
- Session Key created
- Expiration time reached or spending_limit exhausted

**Participating Roles**: MCP Server, Solana Blockchain

**Detailed Steps**:

1. MCP Server attempts to execute payment using Session Key
2. On-chain verification fails:
   - `current_slot >= session_expires_at` (expired)
   - Or `current_usage + payment_amount > spending_limit` (quota exhausted)
3. On-chain transaction rejected
4. MCP returns error to Agent
5. **Self-funded Mode Recovery** (optional):
   - User can execute `CloseSession` instruction
   - Return remaining SOL from ephemeral key to main wallet

**Expected Results**:
- Payment rejected
- Session Key no longer usable
- Remaining Gas fee can be returned via CloseSession

**Exception Handling**:
- Need to re-trigger authorization flow to create new Session Key

---

### Use Case 5.5: Create Sponsored Mode Session Key (sponsored)

**Prerequisites**:
- MCP Server configured with `pay_mode = "sponsored"`
- Project Relayer wallet has sufficient SOL balance
- User has authorized payment on mobile (Use Case 4.2 steps 7-8)

**Participating Roles**: Sentinel App, MCP Server, Relayer Wallet, Solana Blockchain

**Detailed Steps**:

1. User clicks Approve on ChallengeScreen (Use Case 4.2 step 8)
2. App calls Rust bridge `create_session_key_for_payment()`:
   - Generate Ed25519 ephemeral key pair
3. Build on-chain SessionToken PDA registration transaction:
   - `owner`: User main wallet public key
   - `ephemeral_pubkey`: Ephemeral key public key
   - `expiry` / `scope` / `spending_limit`: Same as Use Case 5.1
4. **Difference from self-funded mode**: Transaction `feePayer` is Relayer wallet (not ephemeral key)
5. Relayer wallet signs and submits transaction to Solana RPC
6. On-chain confirmation
7. App returns `session_key_pubkey` and `chain_tx_signature` to MCP Server
8. Subsequent payments signed by Session Key, Gas borne by Relayer

**Expected Results**:
- Session Key registered on-chain
- User does not need to pre-fund Gas
- Ephemeral key address does not need to hold SOL

**Exception Handling**:
- Relayer wallet balance insufficient -> on-chain transaction fails, MCP returns error
- Relayer service unreachable -> fallback to self-funded mode or return error
---

<!-- State Channel: Exploration phase, not enabled
## Business Event 6: State Channel Opening

### Use Case 6.1: User App Selects Hub → Requests MCP to Create Channel via DIDComm

**Preconditions**:
- User App and MCP Server are paired (Use Case 2.1)
- Hub Registry service is available
- At least one active Hub is registered

**Participants**: User, Sentinel App, MCP Server, Hub Registry, Channel Hub

**Detailed Steps**:

1. User selects "Open Channel" action in the App
2. App calls Hub Registry API `GET /v1/hubs?status=active` to retrieve available Hub list
3. App displays Hub list for user selection (showing name, latency, fee rate, liquidity)
4. User selects a Hub and configures parameters:
   - `deposit`: Deposit amount (lamports)
   - `token_mint`: Token address (default SOL)
   - `tree_depth`: Merkle tree depth (default 4)
5. App constructs `create-channel-request` DIDComm message:
   ```json
   {
     "hub_endpoint": "http://hub:3003",
     "provider_pubkey": "Base58SolanaPubkey",
     "token_mint": "So11111111111111111111111111111111",
     "deposit": 1000000000,
     "tree_depth": 8
   }
   ```
6. JWE encrypted and sent to MCP Server via Mediator

**Expected Result**:
- App has sent channel creation request to MCP Server

**Exception Handling**:
- Hub Registry unreachable → Display error, unable to retrieve Hub list
- No active Hubs → Prompt that no Hubs are currently available

---

### Use Case 6.2: Merchant App Selects Hub → Requests MCP to Create Channel via DIDComm

**Preconditions**:
- Merchant App and Merchant MCP Server are paired
- Hub is available

**Participants**: Merchant, Merchant App, Merchant MCP, Channel Hub

**Detailed Steps**:
- Same as Use Case 6.1, merchant side creates channel through Merchant MCP Server

**Expected Result**: Same as Use Case 6.1

**Exception Handling**: Same as Use Case 6.1

---

### Use Case 6.3: Channel Creation Succeeded

**Preconditions**:
- MCP Server has received `create-channel-request`

**Participants**: MCP Server, Channel Hub

**Detailed Steps**:

1. MCP Server decrypts `create-channel-request`
2. MCP calls Channel Hub HTTP API `POST /v1/channels/open`:
   ```json
   { "provider_pubkey": "...", "token_mint": "...", "deposit": 1000000000, "tree_depth": 8 }
   ```
3. Channel Hub processes:
   - Generates channel_id (32-byte random)
   - Creates ChannelManager instance
   - Initializes Merkle Tree
   - Creates initial UTXO leaf (deposit amount)
   - Persists to sled
4. Hub returns:
   ```json
   { "channel_id": "hex_encoded_32_bytes", "sequence": 0, "current_root": "hex_encoded_root" }
   ```
5. MCP constructs `create-channel-response` DIDComm message (JWE encrypted):
   ```json
   { "channel_id": "hex_encoded_32_bytes", "sequence": 0, "current_root": "hex_encoded_root", "success": true }
   ```
6. Sent back to App via Mediator
7. App receives response, updates UI to display new channel

**Expected Result**:
- State channel created successfully
- App obtains channel_id and initial root
- Channel status is Open

**Exception Handling**:
- Hub returns error → MCP forwards error to App

---

### Use Case 6.4: Channel Creation Failed

**Preconditions**: Same as Use Case 6.3

**Participants**: MCP Server, Channel Hub, App

**Detailed Steps**:

1. MCP calls Hub API to create channel
2. Hub returns error (e.g., insufficient deposit, invalid parameters)
3. MCP constructs `create-channel-response`:
   ```json
   { "channel_id": "", "sequence": 0, "current_root": "", "success": false, "error_message": "Failed to open channel" }
   ```
4. Sent back to App
5. App displays error message

**Expected Result**: App displays reason for channel creation failure

**Exception Handling**:
- Hub unreachable → MCP returns network error
- User can modify parameters and retry

---
-->

<!-- State Channel: Exploration phase, not enabled
## Business Event 7: State Channel Off-Chain Payment

### Use Case 7.1: User App Initiates Payment → MCP Executes LeafUpdate + CoSign via Hub

**Preconditions**:
- User has an Open state channel with Hub
- Channel balance is sufficient

**Participants**: Sentinel App, MCP Server, Channel Hub

**Detailed Steps**:

1. User initiates payment in the App (enters amount and recipient)
2. App constructs `channel-payment-request` DIDComm message
3. Sent to MCP Server via Mediator
4. MCP calls Hub API `POST /v1/channels/{id}/pay`:
   ```json
   { "amount": 100000000, "recipient_pubkey": "..." }
   ```
5. Channel Hub executes payment:
   - Creates LeafUpdate (type: Transfer)
   - Deducts amount from payer UTXO
   - Adds amount to payee UTXO
   - Updates Merkle Tree
   - Generates SignedState
   - Requests CoSign from both parties
6. Hub returns:
   ```json
   { "sequence": 1, "leaf_index": 2, "new_root": "hex_encoded_root" }
   ```
7. MCP sends `channel-payment-confirm` DIDComm message to Merchant App
8. MCP returns payment result to User App

**Expected Result**:
- Off-chain payment executed successfully
- Merkle Tree updated
- Both parties signed new state

**Exception Handling**:
- Insufficient channel balance → Returns InsufficientBalance error
- Channel closed → Returns ChannelClosed error
- CoSign failed → Roll back this update

---

### Use Case 7.2: Batch Payment Pipeline

**Preconditions**:
- Channel status is Open
- Multiple consecutive operations need to be executed

**Participants**: MCP Server, Channel Hub

**Detailed Steps**:

1. Use Pipeline builder to batch-create multiple LeafUpdates:
   ```rust
   let mut pipeline = Pipeline::new(channel_id);
   pipeline.add_transfer(payer_a, payer_b, 1000)?;
   pipeline.add_transfer(payer_a, payer_b, 2000)?;
   pipeline.add_htlc_create(payer_a, payer_b, 500, hash_lock, timelock)?;
   ```
2. Execute Pipeline:
   - Apply all LeafUpdates in order
   - All succeed → Update Merkle Tree, generate new state
   - Any failure → Automatically roll back all updates
3. Request CoSign from both parties
4. Return final sequence and root

**Expected Result**:
- Batch operations execute atomically (all succeed or all roll back)
- Merkle Tree is updated only once

**Exception Handling**:
- Any operation in the Pipeline fails → All roll back

---

### Use Case 7.3: HTLC Payment (Conditional Payment)

**Preconditions**:
- Channel status is Open (→ Use Case 6.3)
- Initiator and recipient are in the same channel or have a routing path (→ Use Case 11.1)

**Participants**: MCP Server, Channel Hub

**Detailed Steps**:

1. **Create HTLC**:
   - Initiator creates HTLC Leaf:
     - `hash_lock`: SHA-256(preimage) hash
     - `timelock`: Expiry slot
     - `amount`: Locked amount
   - Update Merkle Tree
   - CoSign by both parties
2. **Reveal Preimage (Unlock)**:
   - Recipient provides preimage
   - Verify `SHA-256(preimage) == hash_lock`
   - Locked amount transferred to recipient
3. **Timeout Refund**:
   - If timelock is reached but preimage is not revealed
   - Locked amount refunded to initiator

**Expected Result**:
- HTLC conditional payment executed successfully
- Or automatic refund after timeout

**Exception Handling**:
- Preimage mismatch → HTLC cannot be unlocked
- Timelock expired → Automatic refund

---
-->

## Business Event 8: QR Code Payment Collection

### Use Case 8.1: Merchant App Generates Payment QR Code

**Preconditions**:
- Merchant has generated identity (Use Case 1.2)
- Hub Endpoint is configured

**Participants**: Merchant, Merchant App

**Detailed Steps**:

1. Merchant opens Merchant App, enters QR Generate page
2. Enters amount (USDC) + optional description (e.g., "Coffee")
3. App calls Rust bridge `generate_payment_qr()`:
   - Generates UUID v4 as order_id
   - Creates order (status: pending)
   - Persists to sled
4. Constructs PaymentQrData:
   ```json
   {
     "type": "ignite-pay-request",
     "version": 1,
     "merchant_did": "did:ignite:...",
     "amount": 1000000000,
     "description": "Coffee",
     "order_id": "uuid-v4",
     "hub_endpoint": "http://hub:3003",
     "timestamp": 1713700000
   }
   ```
5. Base64URL encodes and generates QR code string: `ignite://pay?d=<base64url(JSON)>`
6. Displays QR code, enters waiting-for-confirmation state
7. Starts dual-channel waiting:
   - Primary channel: Listen to `MerchantPushService.confirmations` stream
   - Fallback polling: Call `refreshOrders()` every 5 seconds to check order status

**Expected Result**:
- QR code displayed on screen
- Order created (pending status)
- Waiting for user to scan and pay

**Exception Handling**:
- QR code generation failed → Display error
- Order creation failed → Prompt retry

---

<!-- State Channel: Exploration phase, not enabled
### Use Case 8.2: User App Scans Merchant QR Code → Initiates State Channel Payment

**Preconditions**:
- User has an Open state channel (via Hub)
- User App's Sentinel is opened

**Participants**: User, Sentinel App, Channel Hub

**Detailed Steps**:

1. User opens Sentinel App
2. Scans merchant QR code
3. App calls Rust bridge `parse_payment_qr()` to parse PaymentQrData
4. Displays QrPaymentScreen confirmation page:
   - Merchant DID / Name
   - Amount (USDC display)
   - Description
5. User confirms payment
6. App calls Rust bridge `channel_pay()`:
   - Internally calls Hub API `POST /v1/channels/{id}/pay`
7. Hub processes payment (→ Use Case 7.1)
8. Returns payment result: sequence, leaf_index
9. App displays payment success result

**Expected Result**:
- Payment executed successfully
- User sees payment confirmation

**Exception Handling**:
- No available channel → Prompt to open channel first
- Insufficient channel balance → Prompt insufficient balance
- Hub unreachable → Prompt network error

---

### Use Case 8.3: Merchant App Receives Payment Confirmation → Voice Announcement

**Preconditions**:
- User has completed payment (Use Case 8.2)
- Merchant App is online (WS or FCM)

**Participants**: Channel Hub, Mediator, Merchant App

**Detailed Steps**:

1. After Hub processes payment, constructs `channel-payment-confirm` DIDComm message (JWE encrypted):
   ```json
   { "order_id": "uuid-v4", "channel_id": "hex...", "leaf_index": 2, "sequence": 1, "amount": 1000000000 }
   ```
2. Hub sends to Merchant App via Mediator:
   - Domestic: WS push
   - Overseas: FCM signal → HTTPS pull
3. Merchant App receives message:
   - Calls Rust `decrypt_message()` to decrypt JWE
   - Extracts order_id, channel_id, leaf_index, sequence
4. Calls Rust `confirm_order()` to update order status: pending → confirmed
5. Triggers the following actions:
   - QR page displays green checkmark ✓
   - Haptic Feedback
   - Voice announcement: "Payment received 1.00 USDC" (bilingual Chinese/English, depending on settings)
   - Dashboard today's summary refresh
6. If primary channel does not receive confirmation:
   - Fallback polling detects status change via `refreshOrders()` after 5 seconds
   - Executes the same confirmation flow

**Expected Result**:
- Order status updated to confirmed
- Merchant receives voice announcement
- QR page displays success indicator

**Exception Handling**:
- Message decryption failed → Ignore message
- Order confirmation failed → Log error, wait for next polling
- Push delay → Fallback polling ensures no missed notifications

---
-->

### Use Case 8.4: AI Agent Generates Payment QR Code via Merchant MCP

**Preconditions**:
- Merchant MCP Server is running
- Merchant Hub Endpoint is configured

**Participants**: AI Agent, Merchant MCP, Channel Hub, User

**Detailed Steps**:

1. AI Agent (merchant side) calls Merchant MCP Tool `generate_payment_qr`:
   - Input: `amount = 1000000000` (1 USDC), `description = "Coffee"`
   - Optional input: `order_id` (auto-generates UUID if not provided)
2. Merchant MCP generates PaymentOrder (status: pending), persists to sled
3. Constructs PaymentQrData and encodes as QR string: `ignite://pay?d=<base64url(JSON)>`
4. Returns QR text and ASCII QR code to Agent
5. Agent displays QR code to user (terminal/webpage/print)
6. User scans with Sentinel App (triggers Use Case 8.2)
7. Merchant App or Agent can call `check_payment(order_id)` to poll order status
8. After payment is completed (Use Case 8.3), Merchant MCP audit log records the transaction

**Expected Result**:
- AI Agent obtains QR code, available for user to scan and pay
- Order has been created in Merchant MCP
- Order status automatically updates after payment is completed

**Exception Handling**:
- Hub Endpoint unreachable → QR code can still be generated, but payment will fail when user scans
- Order already exists (duplicate order_id) → Overwrite and update the original order

---

<!-- State Channel: Exploration phase, not enabled
## Business Event 9: State Channel Closure and Settlement

### Use Case 9.1: Cooperative Close Channel

**Preconditions**:
- Channel status is Open
- Both parties agree to close

**Participants**: User/Merchant, Channel Hub, Solana Blockchain

**Detailed Steps**:

1. Either party initiates close request (App or MCP)
2. Calls Hub API `POST /v1/channels/{id}/close`
3. Hub executes cooperative close:
   - Both parties sign final state (Latest SignedState)
   - Constructs on-chain settlement transaction
   - Submits to Solana
4. On-chain program verifies dual signatures
5. Executes fund distribution
6. Channel status changes to Closed

**Expected Result**:
- Channel closed
- Funds distributed according to final state
- On-chain settlement confirmed

**Exception Handling**:
- One party refuses to sign → Can switch to unilateral close (→ Use Case 9.2)
- On-chain submission failed → Retry

---

### Use Case 9.2: Unilateral Close Channel

**Preconditions**:
- Channel status is Open
- One party wishes to close but cannot contact the other party

**Participants**: Initiator, Channel Hub, Solana Blockchain

**Detailed Steps**:

1. Initiator directly submits on-chain close transaction
2. On-chain program records close request
3. Enters challenge period (`challenge_duration` slots, default 5000 slots)
4. During challenge period, the other party can:
   - Submit dispute (Use Case 9.3)
   - Or not respond
5. After challenge period ends, proceeds to settlement

**Expected Result**:
- Channel enters closure process
- Challenge period countdown begins

**Exception Handling**:
- Other party submits dispute during challenge period → Enter dispute resolution (→ Use Case 9.3)

---

### Use Case 9.3: Dispute Resolution

**Preconditions**:
- Channel is in challenge period (Use Case 9.2)
- Other party has a more recent state

**Participants**: Disputing party, Solana Blockchain

**Detailed Steps**:

1. Disputing party submits dispute within challenge period
2. Submits latest SignedState as evidence
3. On-chain program verifies:
   - SignedState contains valid signatures from both parties
   - SignedState's sequence is more recent than current on-chain state
4. On-chain program selects the superior (higher sequence) state
5. Updates on-chain state
6. Proceeds to settlement

**Expected Result**:
- On-chain adopts the latest dual-signed state
- Fair fund distribution

**Exception Handling**:
- Invalid evidence (signature mismatch) → Dispute rejected
- Sequence not higher → Dispute rejected

---

### Use Case 9.4: On-Chain Settlement and Claim (Settle + Claim + Finalize)

**Preconditions**:
- Challenge period has ended (after cooperative close or dispute resolution)

**Participants**: All parties, Channel Hub, Solana Blockchain

**Detailed Steps**:

1. **Settle**: Call Hub API `POST /v1/channels/{id}/settle`
   - Submit on-chain settlement transaction
   - On-chain program verifies challenge period has elapsed
   - Lock channel final state
2. **Claim**: Each party calls `POST /v1/channels/{id}/claim`
   - Submits respective Merkle Proof (proving UTXO leaf ownership)
   - On-chain verifies Proof
   - Transfers corresponding funds from Escrow to each party's address
3. **Finalize**: Call `POST /v1/channels/{id}/finalize`
   - Clean up on-chain PDA accounts
   - Release storage space

**Expected Result**:
- Funds distributed to each party's address according to final state
- Channel PDA cleaned up

**Exception Handling**:
- Invalid Proof during Claim → Claim fails
- Some leaves unclaimed → Can retry later

---
-->

<!-- State Channel: Exploration phase, not enabled
## Business Event 10: Hub Registration and Discovery

### Use Case 10.1: Channel Hub Auto-Registers to Hub Registry on Startup

**Preconditions**:
- Hub Registry service running on `:3004` (PostgreSQL available)
- Channel Hub has configured `[hub_registry]` section

**Participants**: Channel Hub, Hub Registry

**Detailed Steps**:

1. Channel Hub starts up
2. Reads `[hub_registry]` section from configuration:
   ```toml
   [hub_registry]
   url = "http://localhost:3004"
   publish_interval_secs = 60
   ```
3. Hub calls Hub Registry API `POST /v1/hubs` to register itself:
   ```json
   {
     "hub_did": "did:ignite:z...",
     "endpoint_url": "http://hub:3003",
     "name": "Hub-ABC12345",
     "description": "Main payment hub",
     "active_pubkey": "Base58SolanaPubkey",
     "collateral": 100000000000,
     "available_liquidity": 50000000000,
     "fee_rate_bps": 10,
     "supported_tokens": ["So11111111111111111111111111111111"]
   }
   ```
4. Registry returns `hub_id` (UUID)
5. Hub saves `hub_id` for subsequent metric updates

**Expected Result**:
- Hub registered to Registry
- Hub obtains hub_id
- Hub can be discovered by App queries

**Exception Handling**:
- Registry unreachable → Hub can still run, but cannot be discovered
- hub_did already exists → Update existing record

---

### Use Case 10.2: Hub Periodically Updates Performance Metrics to Registry

**Preconditions**:
- Hub is registered (Use Case 10.1)

**Participants**: Channel Hub, Hub Registry

**Detailed Steps**:

1. Hub triggers metric update every `publish_interval_secs` (default 60 seconds)
2. Hub collects current metrics:
   - `online_rate`: Online percentage (0-100)
   - `success_rate`: Success rate (0-100)
   - `avg_latency_ms`: Average latency
   - `active_channels`: Number of active channels
   - `available_liquidity`: Available liquidity
   - `fee_rate_bps`: Fee rate (basis points)
3. Calls Registry API `PUT /v1/hubs/{hub_id}/metrics`
4. Registry updates database

**Expected Result**:
- Hub metrics updated in real time
- App can query latest Hub performance data

**Exception Handling**:
- Registry unreachable → Retry next time
- Metric collection failed → Use previous data

---

### Use Case 10.3: App Queries Hub Registry to Get Available Hub List

**Preconditions**:
- Hub Registry service is available
- At least one Hub is registered

**Participants**: App, Hub Registry

**Detailed Steps**:

1. App calls Registry API `GET /v1/hubs?status=active&limit=100&offset=0`
2. Optional filter parameters:
   - `status=active`: Only return active Hubs
   - `token_mint=So111111...`: Filter by supported token
3. Registry returns Hub list:
   ```json
   {
     "hubs": [
       {
         "hub_id": "uuid",
         "name": "Hub-ABC12345",
         "endpoint_url": "http://hub:3003",
         "fee_rate_bps": 10,
         "available_liquidity": 50000000000,
         "online_rate": 100,
         "success_rate": 99,
         "avg_latency_ms": 50,
         "active_channels": 42,
         "supported_tokens": ["So111111..."]
       }
     ]
   }
   ```
4. App displays Hub list for user selection

**Expected Result**:
- App displays available Hub list
- User can select Hub based on latency, fee rate, liquidity, etc.

**Exception Handling**:
- Registry unreachable → Display error
- No Hubs returned → Prompt no Hubs currently available

---

### Use Case 10.4: Hub Deregistration (Offline)

**Preconditions**:
- Hub is registered

**Participants**: Channel Hub, Hub Registry

**Detailed Steps**:

1. Hub calls Registry API `DELETE /v1/hubs/{hub_id}`
2. Registry sets Hub status to `inactive`
3. Subsequent queries with `status=active` will not return this Hub

**Expected Result**:
- Hub status becomes inactive
- No longer discoverable by App

**Exception Handling**:
- Hub goes offline unexpectedly without deregistration → Admin can manually deregister via Registry API

---
-->

<!-- State Channel: Exploration phase, not enabled
## Business Event 11: Multi-Hop Routing Payment

### Use Case 11.1: User Pays via Multi-Hop Path User → Hub A → Hub B → Merchant

**Preconditions**:
- User has a channel with Hub A
- Hub A has a channel with Hub B
- Hub B has a channel with Merchant
- Routing path is available

**Participants**: User App, Hub A, Hub B, Merchant

**Detailed Steps**:

1. User initiates cross-Hub payment request
2. RouteService executes DFS route discovery:
   - Starts search from the Hub where the user is located
   - Finds a path to the Hub where the merchant is located
   - Scores the path (based on liquidity, fee rate, latency)
3. Selects optimal path: User → Hub A → Hub B → Merchant
4. MultiHopManager constructs multi-hop payment:
   - Decreasing timelock per hop (e.g., Hub B's timelock < Hub A's timelock)
   - First hop: User → Hub A: Create HTLC (hash_lock, timelock_T1)
   - Second hop: Hub A → Hub B: Create HTLC (hash_lock, timelock_T2 < T1)
   - Third hop: Hub B → Merchant: Create HTLC (hash_lock, timelock_T3 < T2)
5. Merchant reveals preimage to unlock the last hop
6. Preimage propagates backward along the path, unlocking hop by hop
7. Payment completed

**Expected Result**:
- Multi-hop payment successful
- Each intermediate Hub earns relay fee
- Funds securely reach merchant

**Exception Handling**:
- HTLC creation failed at any hop → Overall payment fails
- Timeout without unlock → → Use Case 11.3

---
### Use Case 11.2: Route Discovery Failure -> Fallback to Direct Channel

**Preconditions**:
- No available multi-hop route path
- User has a direct channel with the target

**Participating Roles**: User App, Channel Hub

**Detailed Steps**:

1. RouteService fails to find a path (no reachable path)
2. System checks if there is a direct channel to the target
3. If a direct channel exists:
   - Execute standard payment through the direct channel (Use Case 7.1)
4. If no direct channel exists:
   - Return route unreachable error

**Expected Result**:
- Fallback to direct channel payment
- Or return no available path error

**Exception Handling**:
- Direct channel balance insufficient -> Return error

---

### Use Case 11.3: HTLC Timeout -> Automatic Refund

**Preconditions**:
- HTLC for one hop in a multi-hop payment has been created
- Timelock reached but preimage not revealed

**Participating Roles**: Channel Hub, Solana Blockchain

**Detailed Steps**:

1. Timelock slot reached
2. HTLC state changes to Expired
3. Locked amount automatically refunded to the initiator
4. Outer HTLCs also time out due to inner refund, cascading refunds layer by layer
5. Ultimately all locked funds are refunded to the original initiator

**Expected Result**:
- All HTLC locked funds refunded
- No fund loss occurs

**Exception Handling**:
- On-chain failure during refund -> Retry

---
-->

## Business Event 12: Merchant DID Onboarding

### Use Case 12.1: Platform Issues Verifiable Credential for Merchant

**Preconditions**:
- Merchant has generated DID identity
- Merchant has submitted identity verification documents

**Participating Roles**: Merchant, Platform

**Detailed Steps**:

1. Merchant submits to the platform:
   - `did:ignite` identifier
   - Solana receiving public key
   - Identity verification documents
   - Service metadata (name, type, description)
2. Merchant signs the request with DID private key: `issue_vc:{did}:{merchant_name}:{nonce}`
3. Platform verifies the signature (confirming DID ownership)
4. Platform reviews merchant documents
5. After review passes, platform issues VC:
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
       "proofValue": "<ed25519_signature_base58>"
     }
   }
   ```
6. Platform returns VC to the merchant

**Expected Result**:
- Merchant receives a valid VC issued by the platform
- VC contains Ed25519Signature2020 proof

**Exception Handling**:
- Review rejected -> Platform returns rejection reason
- Signature verification failed -> Request re-signing and resubmission

---

### Use Case 12.2: Merchant DID Registered to On-Chain Merkle Tree

**Preconditions**:
- Merchant has obtained platform-issued VC (Use Case 12.1)
- Concurrent Merkle Tree has been deployed

**Participating Roles**: Platform, Solana Blockchain

**Detailed Steps**:

1. Platform computes MerchantLeaf:
   - `merchant_did_hash = SHA-256(DID public key)`
   - `active_pubkey = Solana receiving address`
   - `platform_vc_hash = SHA-256(canonical_json(VC))`
   - `status = 0` (active)
2. Platform computes PDA index: `Index = Hash(Program_ID + Original_PK)`
3. Platform calls `append` instruction to insert the leaf into the Concurrent Merkle Tree:
   - Tree parameters: maxDepth=14, maxBufferSize=64
   - Supports ~16K merchants
4. On-chain program verifies:
   - Platform signature validity (PlatformConfig PDA, seeds: `[b"platform-config"]`)
   - Subject Binding: `credential_subject_pk == signer.key()`
5. Indexer generates Merkle Proof
6. Leaf node can be queried and verified on-chain

**Expected Result**:
- Merchant identity is on-chain
- Verifiable through Merkle Proof
- MerchantLeaf status = 0 (active)

**Exception Handling**:
- Leaf already exists -> Use `replace_leaf` to update
- On-chain transaction failed -> Retry

---

### Use Case 12.3: MCP Server Verifies Merchant On-Chain Identity

**Preconditions**:
- MCP Server receives X402 payment request
- Merchant is on-chain (Use Case 12.2)

**Participating Roles**: MCP Server, Solana Blockchain

**Detailed Steps**:

1. MCP Server extracts `merchant_did` from 402 response
2. **On-Chain Merkle Proof Verification**:
   - Obtain Merkle Proof via Helius DAS API (IndexerClient)
   - Local `verify_proof_locally()`: compute Proof + Leaf == Root
   - Check `MerchantLeaf.status == 0` (active)
3. **VC Signature Verification**:
   - Extract merchant VC from 402 response
   - Verify Ed25519Signature2020 proof using built-in platform public key
   - Check `expirationDate` has not expired
4. **Consistency Check**:
   - DID public key hash of `credentialSubject.id` in VC == on-chain `merchant_did_hash`
5. All checks pass -> Proceed to payment decision flow
6. Any check fails -> Reject payment

**Expected Result**:
- Merchant identity verification passed
- Confirmed merchant is on-chain with active status
- VC is valid and consistent with on-chain data

**Exception Handling**:
- Merkle Proof retrieval failed -> Reject payment
- VC signature invalid -> Reject payment
- Consistency mismatch -> Reject payment (possible identity impersonation)
- On-chain status != 0 -> Reject (merchant revoked)

---

## Business Event 13: Message Push

### Use Case 13.1: Overseas Users -> FCM Push Signal + HTTPS Pull

**Preconditions**:
- User App has registered FCM token (`push_channel: "fcm"`)
- MCP Server and Mediator WebSocket connection is normal

**Participating Roles**: MCP Server, Mediator, Google FCM, Sentinel App

**Detailed Steps**:

**Uplink (MCP -> Phone)**:

1. MCP Server constructs payment authorization request `payment-auth-request` (JWE encrypted)
2. Sends to Mediator via WebSocket
3. Mediator receives and stores in message queue, generates `msg_id`
4. Mediator queries user's `push_channel` preference: `"fcm"`
5. Mediator calls FCM to send Data Message:
   ```json
   { "type": "SIGNAL", "msg_id": "uuid-123" }
   ```
6. FCM pushes to user's phone
7. Phone receives FCM message:
   - Foreground: `FirebaseMessaging.onMessage` triggers
   - Background: `FirebaseMessaging.onBackgroundMessage` triggers
8. App calls `GET /v1/sync/messages/{msg_id}` to pull full JWE
9. App performs DIDComm Unpack (decrypt)
10. Display payment authorization interface

**Downlink (Phone -> MCP)**:

1. After user authorizes, App constructs response JWE
2. Submits to Mediator via `POST /v1/agents/{agent_id}/command`
3. Mediator forwards to MCP Server via WebSocket

**Expected Result**:
- Message successfully delivered via FCM signal + HTTPS pull
- User can receive payment authorization request in real-time

**Exception Handling**:
- FCM signal lost -> App triggers `GET /v1/sync/list` fallback sync when returning to foreground
- iOS Force Quit -> Push may be delayed, sync completes after returning to foreground

---

### Use Case 13.2: Domestic Users -> WebSocket Direct Push

**Preconditions**:
- User App has registered `push_channel: "websocket"`
- App's WebSocket connection with Mediator is online

**Participating Roles**: MCP Server, Mediator, Sentinel App

**Detailed Steps**:

**Online Push**:

1. MCP Server constructs payment authorization request (JWE encrypted)
2. Sends to Mediator via WebSocket
3. Mediator queries user's `push_channel` preference: `"websocket"`
4. Mediator checks if user's WebSocket session is online
5. **Online**: Directly pushes JWE to phone via WebSocket
6. Phone App receives in real-time via `onWebSocketMessage`
7. DIDComm Unpack decrypt -> Display

**Offline Temp Storage**:

1. If step 4 detects WS offline:
2. Mediator temporarily stores message in message queue
3. After phone App comes back online:
   - Perform Mediator handshake (`mediate-request` -> `keylist-update`)
   - Send `messagepickup/3.0/status-request`
   - Receive `status` (returns queued message count)
   - Send `messagepickup/3.0/batch-pickup` for batch retrieval
   - Receive `batch` returning batch messages
4. Process each message by decrypting individually

**Expected Result**:
- Messages pushed in real-time when online
- Messages stored temporarily when offline, batch retrieved after reconnection

**Exception Handling**:
- WS connection unstable -> Auto-reconnect (3 second delay)
- Batch retrieval failed -> Retry

---

### Use Case 13.3: Offline Messages -> Pickup Retrieval After Reconnection

**Preconditions**:
- App has been offline for a period
- Mediator has stored offline messages

**Participating Roles**: App, Mediator

**Detailed Steps**:

1. App returns to foreground or network recovers
2. App automatically triggers sync:
   - Priority: pull via Message Pickup 3.0 protocol
   - Fallback: call `GET /v1/sync/list?after={last_read_id}&limit=100`
3. Retrieve all unread messages during offline period
4. Process each message by decrypting individually
5. Update `last_read_id` cursor

**Full Data Loss Recovery**:

1. If App lost local data (reinstall/device change)
2. Use `GET /v1/sync/list?after=&limit=100` (no after parameter)
3. Sync starting from earliest messages
4. Server filters by `user_did`, ensuring only that user's messages are returned

**Expected Result**:
- All offline messages have been retrieved and processed
- No messages missed

**Exception Handling**:
- Offline messages older than 7 days may have been purged -> Rely on business layer retry
- Cursor lost -> Sync from the beginning

---

### Use Case 13.4: App Returns to Foreground -> Fallback Sync

**Preconditions**:
- App was in background or lock screen state
- There may be unread DIDComm messages

**Participating Roles**: App, Mediator

**Detailed Steps**:

1. App returns to foreground from background (AppLifecycleState.resumed)
2. App automatically triggers fallback sync (similar to Use Case 13.3, but different trigger reason):
   - WebSocket still online: send `messagepickup/3.0/status-request` to check for unread messages
   - WebSocket disconnected: reconnect first (3 second delay), then perform full handshake + Pickup retrieval
3. If Pickup protocol unavailable, fallback to HTTPS pull:
   - Call `GET /v1/sync/list?after={last_read_id}&limit=100`
4. After receiving messages, perform deduplication:
   - Each message has a unique `id` (DIDComm Message ID)
   - Check if the `id` has already been processed locally
   - Already processed -> Skip
   - Not processed -> Decrypt, process, update cursor
5. Processing complete, UI refreshes (e.g., pending payment authorization popup)

**Deduplication Guarantee**:

| Layer | Mechanism | Description |
|:-----|:----------|:------------|
| Message Layer | DIDComm Message `id` | Globally unique, prevents replay |
| Sync Layer | `last_read_id` cursor | Prevents re-fetching already processed messages |
| App Layer | Local processed message cache | Pickup protocol and HTTPS pull may return overlapping messages |

**Expected Result**:
- App syncs all unread messages immediately after returning to foreground
- No omissions, no duplicate processing

**Exception Handling**:
- Sync failed -> Retry on next foreground switch
- Mediator unreachable -> Stay offline, wait for network recovery

## Business Event 14: Merchant DID Lifecycle Management

### Use Case 14.1: Merchant Updates On-Chain VC Hash

**Preconditions**:
- Merchant has completed on-chain registration (Use Case 12.2)
- Platform has issued a new VC (e.g., business scope change, annual review update)

**Participating Roles**: Merchant, did-registry, Solana Blockchain

**Detailed Steps**:

1. Merchant obtains did-registry nonce: `GET /v1/auth/nonce`
2. Merchant signs message with Controller Key: `update-vc:{merchant_did}:{new_vc_hash}:{nonce}`
3. Merchant calls `POST /v1/merchants/update-vc`:
   ```json
   { "merchant_did": "did:ignite:z...", "new_vc_hash": "SHA-256-hash", "signature": "base64", "nonce": "...", "mode": "sponsored" }
   ```
4. did-registry verifies signature and nonce
5. Calls on-chain `update_did_with_vc` instruction:
   - Verifies Controller Key authorization
   - Verifies platform signature
   - Updates the `vc_hash` field of the leaf in the ZK Compression tree
6. On-chain confirmation
7. Subsequent payment verification uses the new `platform_vc_hash`

**Expected Result**:
- On-chain `MerchantLeaf.platform_vc_hash` has been updated
- New VC takes effect for subsequent payment verification

**Exception Handling**:
- Signature mismatch -> Reject (not signed by Controller Key)
- Nonce mismatch -> Reject (anti-replay)
- On-chain transaction failed -> Retry

---

### Use Case 14.2: Merchant Rotates Controller Key

**Preconditions**:
- Merchant has completed on-chain registration
- Merchant holds current Controller Key
- New Ed25519 Controller Key has been generated

**Participating Roles**: Merchant, did-registry, Solana Blockchain

**Detailed Steps**:

1. Merchant generates a new Controller Key locally (Ed25519)
2. Obtain nonce: `GET /v1/auth/nonce`
3. Sign with **current Controller Key**: `rotate-key:{merchant_did}:{new_controller_pubkey}:{nonce}`
4. Call `POST /v1/merchants/rotate-key`:
   ```json
   { "merchant_did": "did:ignite:z...", "new_active_pubkey": "Base58Pubkey", "signature": "base64", "nonce": "..." }
   ```
5. did-registry verifies current Controller Key signature
6. Calls on-chain instruction to update `controller_pk` field
7. Merchant securely stores new Controller Key, destroys old key

**Expected Result**:
- On-chain `MerchantCompressedDid.controller_pk` has been updated
- Old Controller Key is no longer valid
- DID identifier remains unchanged

**Exception Handling**:
- Signature verification failed (not current Controller Key) -> Reject
- New key conflicts with existing key -> Reject

---

### Use Case 14.3: Recover Controller Using Recovery Key

**Preconditions**:
- Merchant has set a Recovery Key (on-chain `recovery_pk != 11111...`)
- Controller Key is lost or compromised

**Participating Roles**: Merchant, did-registry, Solana Blockchain

**Detailed Steps**:

1. Merchant retrieves Recovery Key from cold storage
2. Generate a new Controller Key
3. Obtain nonce: `GET /v1/auth/nonce`
4. Sign recovery message with Recovery Key
5. Call on-chain `recover_controller` instruction:
   - Verify Recovery Key signature
   - Update `controller_pk` to new key
   - Increment `nonce` (anti-replay)
6. On-chain confirmation
7. Merchant uses new Controller Key for subsequent operations

**Expected Result**:
- Controller Key has been reset via Recovery Key
- Merchant can use new Controller Key to manage identity

**Exception Handling**:
- Recovery Key mismatch -> Reject
- Recovery Key also lost -> Identity unrecoverable, need to contact platform

---

### Use Case 14.4: Platform Revokes Merchant VC

**Preconditions**:
- Merchant holds a valid VC
- Platform decides to revoke (e.g., violation, closure)

**Participating Roles**: Platform Administrator, did-registry, Solana Blockchain

**Detailed Steps**:

1. Platform administrator identifies the VC to be revoked (by vc_hash)
2. Obtain nonce: `GET /v1/auth/nonce`
3. Sign with platform signing key: `revoke:{vc_hash}:{nonce}`
4. Call `POST /v1/vc/revoke`:
   ```json
   { "vc_hash": "SHA-256-hash", "reason": "Terms violation", "nonce": "..." }
   ```
5. did-registry verifies platform authority
6. Call on-chain `revoke_vc` instruction:
   - Create `RevokedVc` PDA (seeds: `[b"revoked-vc", vc_hash]`)
   - Record revocation time and reason
7. On-chain confirmation
8. In subsequent payment verification, MCP Server detects VC has been revoked -> Reject payment

**Expected Result**:
- VC has been marked as revoked on-chain
- Merchant's subsequent payment requests are rejected
- `RevokedVc` PDA records revocation information

**Exception Handling**:
- Non-platform administrator call -> Reject (403)
- VC already revoked -> Return AlreadyRevoked error
- On-chain PDA creation failed -> Retry

---

### Use Case 14.5: Merchant Self-Onboarding (SelfOnboard Mode)

**Preconditions**:
- Merchant has obtained platform-issued VC
- Merchant has a Solana wallet and SOL balance
- Merchant chooses self-onboarding (not platform-sponsored)

**Participating Roles**: Merchant, did-registry, Solana Blockchain

**Detailed Steps**:

1. Merchant obtains ZK Proof: `POST /v1/proof`:
   ```json
   { "merchant_did": "did:ignite:z...", "active_pubkey": "Base58", "vc_hash": "SHA-256-hash" }
   ```
2. did-registry returns unsigned on-chain transaction
3. Merchant signs the transaction with their own Solana private key
4. Merchant broadcasts the transaction to Solana RPC themselves
5. On-chain confirmation
6. Merchant calls confirmation endpoint: `POST /v1/merchants/confirm`:
   ```json
   { "did": "did:ignite:z...", "tx_signature": "Base58Sig", "nonce": "..." }
   ```
7. did-registry verifies on-chain transaction
8. Updates local sled record

**Expected Result**:
- Merchant identity has been self-onboarded on-chain
- did-registry has synced the on-chain status

**Exception Handling**:
- On-chain transaction failed -> Merchant retries broadcast
- Confirmation endpoint cannot find transaction -> Prompt merchant to check transaction status
- Confirmation timeout -> Platform does not record, merchant needs to re-confirm

---

### Use Case 14.6: Query Merchant Status and DID Resolution

**Preconditions**:
- Merchant is registered (on-chain or pending confirmation)

**Participating Roles**: Any Client, did-registry

**Detailed Steps**:

1. **Query Merchant Status**: `GET /v1/merchants/status/{did}`
   - Returns: registration status, VC hash, last_updated, on-chain slot
2. **Verify Merchant DID**: `GET /v1/merchants/verify/{did}`
   - Perform full on-chain verification: obtain Merkle Proof -> local verification -> check status
   - Returns: verified (bool), leaf_data, proof_valid
3. **Resolve DID Document**: `GET /v1/did/resolve/{did}`
   - Build W3C DID Document from on-chain and sled data
   - Returns standard DID Document JSON

**Expected Result**:
- Status query returns merchant's current registration status
- Verify endpoint returns on-chain verification result
- DID resolution returns complete DID Document

**Exception Handling**:
- DID does not exist -> Return 404
- On-chain query failed -> Return error message

---

### Use Case 14.7: Query DID Registry Fee Records

**Preconditions**:
- did-registry service is available

**Participating Roles**: Platform Administrator, did-registry

**Detailed Steps**:

1. Call `GET /v1/fees?operation=register&since=1700000000000&limit=50`
2. did-registry queries `fee:{operation}:{timestamp_ms}:{did_hash_hex}` records in sled
3. Returns fee list:
   ```json
   { "fees": [{ "operation": "register", "did": "...", "amount_lamports": 5000, "timestamp_ms": 1700000001000 }] }
   ```

**Expected Result**: Returns fee record list for the specified operation type

**Exception Handling**: No records -> Return empty list

---

<!-- State Channel: Exploration phase, not enabled
## Business Event 15: State Channel Operations

### Use Case 15.1: Channel Fund

**Preconditions**:
- Channel has been created and status is Open
- User needs to increase channel balance

**Participating Roles**: User App, Channel User Service (:3001), Solana Blockchain

**Detailed Steps**:

1. User selects the channel to fund in the App
2. Enter funding amount
3. App calls Channel User API `POST /v1/channels/{id}/fund`:
   ```json
   { "amount": 2000000000 }
   ```
4. Channel User Service processes:
   - Create new UTXO leaf (type: Standard)
   - Update Merkle Tree
   - Generate new SignedState
   - Request dual CoSign
5. Update channel state in sled
6. Return new sequence and root

**Expected Result**:
- Channel balance has been increased
- Merkle Tree updated
- New SignedState has been dual-signed

**Exception Handling**:
- Channel already closed -> Return ChannelClosed error
- Invalid funding amount -> Return InvalidAmount error

---

### Use Case 15.2: UTXO Split (Split Tree)

**Preconditions**:
- Channel is open
- Existing UTXO denominations are unsuitable for subsequent micropayments

**Participating Roles**: Channel User Service

**Detailed Steps**:

1. Call `POST /v1/channels/{id}/split`:
   ```json
   { "leaf_index": 0, "split_amounts": [100000000, 200000000, 700000000] }
   ```
2. Channel User Service executes the split:
   - Select target UTXO leaf
   - Create multiple new sub-denomination leaves
   - Verify denomination sum equals original leaf balance
   - Update Merkle Tree
3. Return new leaf index list

**Expected Result**:
- Original UTXO has been split into multiple UTXOs with specified denominations
- Different denomination leaves can be used for subsequent payments

**Exception Handling**:
- Denomination sum mismatch -> Return ConservationError
- Leaf not found -> Return LeafNotFound

---

### Use Case 15.3: Channel Service WebSocket Authentication

**Preconditions**:
- Channel User/Provider/Hub services are started
- Client needs to receive channel events in real-time

**Participating Roles**: Client (App/MCP), Channel Service

**Detailed Steps**:

1. Client connects WebSocket: `ws://localhost:3001/ws`
2. Send authentication message:
   ```json
   { "type": "auth", "pubkey": "<base58>", "signature": [64 bytes], "timestamp": 1713700000 }
   ```
3. Signature content: `SHA-256("channel-ws-auth:{timestamp}")`
4. Server verifies Ed25519 signature
5. Authentication successful, WebSocket session established
6. Subsequently receive real-time `leaf_update` pushes:
   ```json
   { "type": "leaf_update", "channel_id": "hex", "sequence": 5, "leaf_index": 2 }
   ```
7. Client returns ack confirmation

**Expected Result**:
- WebSocket authentication successful
- Client can receive channel state changes in real-time

**Exception Handling**:
- Signature verification failed -> Server closes WS connection
- Authentication timeout -> Server closes connection

---

### Use Case 15.4: Compliance Status Query

**Preconditions**:
- Channel is open
- Compliance configuration has been set

**Participating Roles**: Channel User Service (:3001)

**Detailed Steps**:

1. Call `GET /v1/compliance/{channel_id}`
2. ComplianceManager returns compliance status:
   ```json
   {
     "channel_id": "hex",
     "window_spending": 500000000,
     "spending_threshold": 1000000000,
     "per_channel_limit": 100000000,
     "travel_rule_triggered": false,
     "window_slots": 100000,
     "current_slot": 250000000
   }
   ```
3. Display total spending within current sliding window and thresholds

**Expected Result**: Returns channel compliance status details

**Exception Handling**: Channel not found -> Return 404

---

### Use Case 15.5: Channel Auto Close

**Preconditions**:
- Channel configured with `auto_close_offset` (Channel Hub: 500000 slots)
- Channel meets auto-close conditions

**Participating Roles**: Channel Hub service, Solana blockchain

**Detailed Steps**:

1. Hub monitors `auto_close_slot` for all channels (= open slot + auto_close_offset)
2. Triggers auto-close when current slot >= `auto_close_slot`
3. Hub initiates cooperative close flow (similar to use case 9.1):
   - Both parties sign the final state
   - Submit on-chain settlement transaction
4. If cooperative close fails (counterparty unresponsive):
   - Fall back to unilateral close (use case 9.2)

**Expected Result**:
- Long-inactive channels are automatically closed
- Funds are settled and returned to each party

**Exception Handling**:
- On-chain submission failure -> retry
- Counterparty unresponsive -> unilateral close

---
-->

<!-- State Channel: Exploration phase, not enabled
## Business Event 16: Hub Network Topology Management

### Use Case 16.1: Hub Local Registration and Information Query

**Preconditions**:
- Channel Hub has started

**Participating Roles**: Channel Hub, other Hubs/clients

**Detailed Steps**:

1. Hub calls `POST /v1/hub/register` to self-register:
   ```json
   {
     "hub_did": "did:ignite:z...",
     "endpoint_url": "http://hub:3003",
     "active_pubkey": "Base58Pubkey",
     "collateral": 100000000000,
     "supported_tokens": ["So11111111111111111111111111111111"]
   }
   ```
2. Hub stores HubLeaf to sled:
   - `hub_did_hash`: SHA-256(Hub DID)
   - `active_pubkey`: collection public key
   - `endpoint_hash`: SHA-256(endpoint URL)
   - `collateral`: collateral amount
   - `platform_vc_hash`: platform VC hash
3. Any client can call `GET /v1/hub/info` to query Hub's own information
4. Call `GET /v1/hub/list` to list all registered Hubs

**Expected Result**: Hub is registered locally and can be queried by other nodes

**Exception Handling**: Duplicate DID registration -> update existing record

---

### Use Case 16.2: Route Edge Management and Graph Refresh

**Preconditions**:
- Multiple Hubs have discovered each other
- Channels have been established between Hubs

**Participating Roles**: Channel Hub administrator, Channel Hub

**Detailed Steps**:

1. Administrator adds route edge: `POST /v1/routes/add-edge`:
   ```json
   {
     "from_hub_did": "did:ignite:z...A",
     "to_hub_did": "did:ignite:z...B",
     "channel_id": "hex",
     "capacity": 5000000000,
     "fee_rate_bps": 5
   }
   ```
2. Hub adds the edge to the routing graph (sled storage)
3. Refresh routing graph: `POST /v1/routes/refresh`:
   - Rescan all channel states
   - Update available capacity
   - Remove edges for closed channels
4. Routing graph can be used for subsequent path discovery (use case 11.1)

**Expected Result**:
- Routing graph updated
- Multi-hop path discovery can use the latest topology

**Exception Handling**:
- Channel does not exist -> reject edge addition
- Hub DID does not exist -> reject

---

### Use Case 16.3: Route Discovery Query

**Preconditions**:
- Routing graph has available edges

**Participating Roles**: Channel Hub

**Detailed Steps**:

1. Call `POST /v1/routes/find`:
   ```json
   { "from_hub_did": "did:ignite:z...A", "to_hub_did": "did:ignite:z...C", "amount": 100000000 }
   ```
2. RouteService executes DFS search:
   - Traverse starting from the source Hub
   - Filter out edges with insufficient capacity
   - Score found paths: `score = 0.3 * fee_score + 0.3 * latency_score + 0.4 * reliability_score`
3. Return optimal path:
   ```json
   { "path": ["hub_A", "hub_B", "hub_C"], "total_fee_bps": 15, "estimated_latency_ms": 120, "score": 0.85 }
   ```

**Expected Result**: Returns optimal routing path and its score

**Exception Handling**: No reachable path -> return empty path list

---

### Use Case 16.4: Hub Relay Multi-hop Payment

**Preconditions**:
- Multi-hop path determined (use case 16.3)
- HTLC for each hop has been calculated

**Participating Roles**: Channel Hub (intermediate node)

**Detailed Steps**:

1. Receive relay request from upstream Hub: `POST /v1/multihop/relay`:
   ```json
   {
     "payment_id": "uuid",
     "from_hub_did": "...",
     "to_hub_did": "...",
     "amount": 100000000,
     "hash_lock": "SHA-256-hash",
     "timelock": 2501000000,
     "hop_index": 2
   }
   ```
2. Hub validates the request
3. Create HTLC in local channel (lock funds)
4. Forward to next-hop Hub
5. Wait for preimage revelation
6. Receive preimage -> unlock local HTLC -> funds received
7. Relay fee automatically credited

**Expected Result**:
- Relay payment executed successfully
- Hub earns relay fee

**Exception Handling**:
- Insufficient channel balance -> return InsufficientCapacity
- Invalid timelock -> return InvalidTimelock
- Downstream Hub unresponsive -> refund after HTLC timeout

---

### Use Case 16.5: Hub Receives Payment

**Preconditions**:
- Hub acts as Provider role
- User initiates payment to Hub through channel

**Participating Roles**: Channel Hub, User

**Detailed Steps**:

1. Receive payment request: `POST /v1/channels/{id}/accept-payment`:
   ```json
   { "leaf_update": { ... }, "signature": "base64" }
   ```
2. Hub validates:
   - LeafUpdate format is correct
   - Signature is valid
   - Amount conservation (total amount unchanged before and after transfer)
3. Accept payment, update Merkle Tree
4. Generate CoSign response

**Expected Result**: Hub accepts payment and completes co-signing

**Exception Handling**:
- Amount not conserved -> return ConservationError
- Invalid signature -> return InvalidSignature

---

### Use Case 16.6: Hub Batch Receive Payments

**Preconditions**: Same as use case 16.5, but multiple payments need processing

**Participating Roles**: Channel Hub

**Detailed Steps**:

1. Receive batch payment request: `POST /v1/channels/{id}/accept-batch`:
   ```json
   { "updates": [ { ... }, { ... } ] }
   ```
2. Hub validates each LeafUpdate individually
3. All valid -> batch update Merkle Tree
4. Any invalid -> reject all (atomicity)
5. Return batch CoSign

**Expected Result**: Batch payment processed atomically and successfully

**Exception Handling**: Any update invalid -> rollback all

---

### Use Case 16.7: Hub Submit Dispute Counter-evidence

**Preconditions**:
- Channel is in challenge period
- Hub has a newer state as counter-evidence

**Participating Roles**: Channel Hub, Solana blockchain

**Detailed Steps**:

1. Hub receives challenge notification
2. Retrieve latest SignedState from sled
3. Call `POST /v1/channels/{id}/submit-counter`:
   ```json
   { "sequence": 15, "root": "hex_root", "signature_a": "base64", "signature_b": "base64" }
   ```
4. Submit on-chain `submit_counter_state` instruction
5. On-chain verifies dual signatures and higher sequence
6. Update on-chain state

**Expected Result**: Hub's counter-evidence is accepted on-chain, channel state updated

**Exception Handling**:
- Sequence not higher -> counter-evidence rejected
- Incomplete signatures -> counter-evidence invalid

---
-->

## Business Event 17: App-side Management and Settings

### Use Case 17.1: Session Key Management and Revocation

**Preconditions**:
- User has created at least one Session Key

**Participating Roles**: User, Sentinel App, Solana blockchain

**Detailed Steps**:

1. User opens SessionKeysScreen
2. App calls Rust bridge to query active Session Key list:
   - Display each Key's public key, creation time, expiration time, spending_limit, used quota
   - Active Keys shown in green, expired Keys shown in gray
3. User selects the Session Key to revoke
4. Confirms revocation operation
5. App calls Rust bridge `revoke_session_key_onchain()`:
   - Build on-chain revocation transaction
   - User signs
   - Submit to Solana
6. After on-chain confirmation, Session Key status becomes Revoked
7. MCP Server's subsequent payments using this Key will be rejected

**Expected Result**:
- Session Key has been revoked on-chain
- App list updates to show revoked status

**Exception Handling**:
- On-chain transaction failure -> prompt to retry
- Key already expired -> no need to revoke, prompt that it has expired

---

### Use Case 17.2: Mnemonic Export and Identity Recovery

**Preconditions**:
- User has generated DID identity

**Participating Roles**: User, Sentinel App

**Detailed Steps**:

**Export Mnemonic**:

1. User opens VaultScreen
2. Clicks "Show Mnemonic Phrase"
3. App requires secondary confirmation (security warning)
4. App calls Rust bridge `exportMnemonicPhrase()`
5. Displays 12 mnemonic words (with animated gradient background)
6. After user manually writes down backup, clicks "I've Saved It"
7. Mnemonic disappears from screen

**Recover Identity from Mnemonic**:

1. User installs Sentinel App on new device
2. Selects "Restore Identity"
3. Inputs 12 mnemonic words
4. App calls Rust bridge `importMnemonicPhrase()`
5. Recover Ed25519 key pair from mnemonic
6. Re-derive DID identifier
7. Connect to Mediator, pull offline messages

**Expected Result**:
- Export: mnemonic securely displayed and then automatically hidden
- Recovery: user restores original DID identity on new device

**Exception Handling**:
- Incorrect mnemonic input -> prompt "Invalid mnemonic"
- Recovered DID differs from original device -> prompt key mismatch

---

### Use Case 17.3: Clear Key Material

**Preconditions**:
- User has generated DID identity

**Participating Roles**: User, Sentinel App

**Detailed Steps**:

1. User opens VaultScreen
2. Clicks "Erase Key Material" (red danger button)
3. App shows confirmation dialog, requiring input of "ERASE" to confirm
4. User enters confirmation text
5. App calls Rust bridge `eraseAllKeyMaterial()`:
   - Delete key pairs from sled database
   - Delete DID Document
   - Delete Session Key cache
   - Delete whitelist/blacklist cache
   - Disconnect Mediator connection
6. App returns to OnboardingScreen

**Expected Result**:
- All key material has been securely erased
- App returns to initial state

**Exception Handling**:
- This operation is irreversible -> ensure user has backed up mnemonic

---

### Use Case 17.4: Solana Network and Program Configuration

**Preconditions**:
- Sentinel App has been initialized

**Participating Roles**: User, Sentinel App

**Detailed Steps**:

1. User opens SettingsScreen
2. Configure Solana network parameters:
   - **Network Selection**: Devnet / Mainnet toggle
   - **RPC URL**: Solana RPC endpoint (editable)
   - **DAS Endpoint**: Helius DAS API endpoint
3. Configure SPL account compression parameters:
   - **Tree Address**: Concurrent Merkle Tree address
   - **Tree Authority**: Tree manager public key
4. Configure Program IDs (read-only display):
   <!-- State Channel: Exploration phase, not enabled
   - State Channel Program ID
   -->
   - DID Program ID
   - Session Key Program ID
5. Select payment mode: self-funded (self_funded) / sponsored (sponsored)
6. App calls Rust bridge to save configuration to sled

**Expected Result**:
- Network configuration updated
- Subsequent operations use new configuration

**Exception Handling**:
- Invalid RPC URL -> connection test failure prompt
- Incorrect Tree Address format -> prompt format error

---

### Use Case 17.5: Audit Log Viewing and IPFS Sync

**Preconditions**:
- User has payment history

**Participating Roles**: User, Sentinel App, IPFS

**Detailed Steps**:

1. User opens VaultScreen, clicks "Audit Logs"
2. App loads audit log list from `LocalLogStore` (SQLite):
   - Each record contains: time, operation type, merchant DID, amount, status
3. App performs IPFS sync in background:
   - `sync_to_ipfs()`: encrypt (E2EE) -> Zstd compress -> upload to IPFS
   - `restore_from_ipfs()`: download from IPFS -> decompress -> decrypt -> merge to local
4. Display sync status (synced/pending sync)

**Expected Result**:
- Audit logs are viewable
- Logs have been synced to IPFS via E2EE encryption

**Exception Handling**:
- IPFS unreachable -> logs saved locally only, marked as pending sync
- Decryption failure -> logs may have been tampered with, flag warning

---

### Use Case 17.6: Merchant-side Order List and Details

**Preconditions**:
- Merchant has order records

**Participating Roles**: Merchant, Merchant App

**Detailed Steps**:

1. Merchant opens PaymentListScreen
2. App calls Rust bridge `list_orders()` to load order list
3. Supports filter tabs: All / Pending Confirmation / Confirmed
4. Pull to refresh
5. Merchant clicks on an order to enter PaymentDetailScreen
6. App calls `get_order(order_id)` to get details:
   - Amount (large font USDC display)
   - Status badge: confirmed=green / pending=amber / failed=red / expired=gray
   - Order number (copyable)
   - Description, Hub endpoint, creation time, confirmation time
   - Channel info (confirmed only): Channel ID, Leaf Index, Sequence

**Expected Result**:
- Merchant can browse and filter all orders
- Can view complete details for each order

**Exception Handling**:
- No orders -> display empty state prompt

---

<!-- State Channel: Exploration phase, not enabled
### Use Case 17.7: Merchant-side Channel List and Operations

**Preconditions**:
- Merchant has opened at least one state channel

**Participating Roles**: Merchant, Merchant App, Channel Hub

**Detailed Steps**:

1. Merchant opens ChannelScreen
2. App calls Rust bridge `merchant_list_channels()` to get channel ID list
3. For each channel, call `merchant_get_channel_status()` to get details:
   - Channel ID, status, Sequence, leaf count, balance, total deposited
4. Display channel card list with top summary: total channels + total balance
5. Merchant clicks on a channel to enter ChannelDetailScreen
6. Display complete channel information
7. Available operations:
   - **Close Channel**: confirmation dialog -> `merchant_close_channel()` -> Hub API `/v1/channels/{id}/close`
   - **Settle**: `merchant_claim_leaf()` -> `merchant_finalize()`

**Expected Result**:
- Merchant can view all channel statuses
- Can initiate close and settle operations from merchant side

**Exception Handling**:
- Hub unreachable -> prompt network error
- Channel already closed -> display Closed status

---
-->

### Use Case 17.8: Merchant Voice Announcement Configuration

**Preconditions**:
- Merchant App is installed

**Participating Roles**: Merchant, Merchant App

**Detailed Steps**:

1. Merchant opens SettingsScreen, finds "Voice Announcement" section
2. Configuration options:
   - **Toggle**: enable/disable voice announcement
   - **Language**: Chinese / English toggle
   - **Volume**: slider adjustment (0-100%)
   - **Test Button**: click to play "Payment received 1.00 USDC" test
3. App calls Flutter TTS (`flutter_tts`) service
4. Configuration persisted to sled

**Expected Result**:
- Voice announcement executes per configuration
- Announces amount in the configured language upon payment confirmation

**Exception Handling**:
- TTS engine unavailable -> degrade to vibration alert only

---

### Use Case 17.9: Manage MCP Connection List

**Preconditions**:
- User has paired at least one MCP Server

**Participating Roles**: User, Sentinel App

**Detailed Steps**:

1. User opens ConnectionScreen
2. App calls Rust bridge `getBoundAgents()` to get paired MCP list
3. Display each MCP connection:
   - MCP DID, label name, connection time, last active time
   - Mediator connection status (WS/FCM channel display)
4. User can:
   - **Add New MCP**: open QR scanner (use case 2.1)
   - **Remove MCP**: call `removeBoundAgent(agent_did)` -> delete peer public key from local cache
5. Pairing relationship updated

**Expected Result**:
- User can view and manage all paired MCP connections
- Can add or remove MCP

**Exception Handling**:
- After removal, MCP can still send messages (until Mediator-side unbinding) -> prompt user

---

<!-- State Channel: Exploration phase, not enabled
## Business Event 18: Compliance and Risk Control

### Use Case 18.1: Sliding Window Spending Threshold Tracking

**Preconditions**:
- Channel User service has configured `[compliance]` section
- Channel has payment records

**Participating Roles**: Channel User service, ComplianceManager

**Detailed Steps**:

1. On each channel payment, ComplianceManager automatically checks:
   - Calculate total spending within current sliding window: sum of all LeafUpdate amounts within `window_slots` range
   - Compare whether `window_spending + new_amount` exceeds `spending_threshold`
2. If threshold exceeded:
   - Reject this payment
   - Return `SpendingThresholdExceeded` error
3. If not exceeded:
   - Record this spending to compliance sled record
   - Allow payment to proceed

**Expected Result**:
- User's spending within sliding window does not exceed `spending_threshold` (1 SOL)
- Excessive payments are automatically rejected

**Exception Handling**:
- Window spending records corrupted -> use conservative estimate (reject payment)

---

### Use Case 18.2: Per-Channel Payment Limit

**Preconditions**:
- Channel User service has configured `per_channel_limit`

**Participating Roles**: Channel User service, ComplianceManager

**Detailed Steps**:

1. User initiates channel payment
2. ComplianceManager checks:
   - Whether current channel cumulative payment amount exceeds `per_channel_limit` (0.1 SOL)
   - Whether this payment amount causes the cumulative value to exceed the limit
3. Exceeds limit -> reject payment
4. Within limit -> allow and record

**Expected Result**: Cumulative payment of a single channel does not exceed the limit

**Exception Handling**: Limit is 0 -> disable this check

---

### Use Case 18.3: Travel Rule Data Collection

**Preconditions**:
- Payment amount exceeds `travel_rule_threshold` (0.5 SOL)
- Identity information of both parties is available

**Participating Roles**: Channel User service, ComplianceManager

**Detailed Steps**:

1. Payment amount > `travel_rule_threshold`
2. ComplianceManager automatically creates Compliance leaf (UTXO type: Compliance):
   - Record sender DID and identity information
   - Record recipient DID and identity information
   - Record amount, time, channel ID
3. Compliance leaf stored in Merkle Tree (tamper-proof)
4. Administrators can query compliance records

**Expected Result**:
- Payments exceeding threshold automatically record Travel Rule data
- Data stored as Compliance leaves in the Merkle Tree

**Exception Handling**:
- Incomplete identity information -> mark as pending supplement

---

### Use Case 18.4: Amount Conservation Verification

**Preconditions**:
- Channel is performing LeafUpdate operation

**Participating Roles**: Channel User/Hub service

**Detailed Steps**:

1. Before each LeafUpdate execution, service automatically verifies amount conservation:
   - Sum all leaf balances (before update)
   - Sum all leaf balances (after update)
   - The two must be equal
2. Conservation verification passed -> allow update
3. Conservation verification failed -> reject update, return `ConservationError`

**Expected Result**:
- All channel operations guarantee amount conservation
- No possibility of creating or destroying funds out of thin air

**Exception Handling**:
- Conservation failure -> transaction rejected + audit log recorded
- May indicate Merkle Tree corruption -> trigger channel close

---

### Use Case 18.5: Pipeline Rollback Mechanism

**Preconditions**:
- Pipeline batch operation is being built
- An operation fails

**Participating Roles**: Channel User/Hub service

**Detailed Steps**:

1. Build Pipeline:
   ```rust
   let mut pipeline = Pipeline::new(channel_id);
   pipeline.transfer_leaf(0, 1, 1000)?;  // success
   pipeline.transfer_leaf(0, 2, 500)?;   // success
   pipeline.create_htlc(0, 3, 200, hash, timelock)?;  // fails (insufficient balance)
   ```
2. Third step returns error
3. Pipeline automatically rolls back LeafUpdates from the first two steps
4. Channel state restored to before Pipeline started
5. Caller receives error information

**Expected Result**:
- Pipeline operation atomicity guaranteed
- On failure, channel state fully rolled back

**Exception Handling**:
- Pipeline dropped without calling `build()` -> automatically calls `abort()` to rollback
-->