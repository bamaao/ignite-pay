# Manual Test Walkthrough

Step-by-step executable walkthrough organized by business flow sequence.
A tester can follow it from start to finish, verifying each flow works correctly before moving to the next.

**Reference docs:**
- [Business Flows](business-flows_en.md) — flow diagrams and code locations
- [Business Scenarios](business-scenarios_en.md) — detailed step descriptions and exception handling
- [App Test Plan](ignite-pay-app-test-plan_en.md) — phone-side UI test cases (TC-M0x-xx)
- [E2E Demo](ecom-demo-end-to-end.md) — e-commerce demo flow

---

## Environment Setup

### Prerequisites

- Solana CLI configured for devnet (`solana config set --url devnet`)
- Docker & Docker Compose installed
- Flutter SDK (for mobile app builds)
- Python 3.10+ (for e-commerce demo)
- WSL or Linux environment for `cargo build-sbf`

### Service Startup

```bash
# 1. Copy and configure environment
make init
# Edit .env with real values

# 2. Start all backend services
make build
make up

# 3. Verify health
make health
```

All services should report OK:
- PostgreSQL, <!-- State Channel: Exploration phase, not enabled - originally included Hub Registry, -->DIDComm Router (user :8080, merchant :4000)
- DID Registry (:8081)<!-- State Channel: Exploration phase, not enabled - originally included Channel User (:3001), Channel Provider (:3002), Channel Hub (:3003) -->

### Key Generation & Funding

```bash
# Generate Solana keypairs for testing
solana-keygen new --outfile test-user.json --no-bip39-passphrase
solana-keygen new --outfile test-merchant.json --no-bip39-passphrase

# Fund on devnet
solana airdrop 2 test-user.json --url devnet
solana airdrop 2 test-merchant.json --url devnet
```

---

## Phase 1: Identity & Pairing (Foundation)

### T1.1 User App — First Launch & DID Creation

**Flow**: Prerequisite for F1
**App Test Case**: TC-M01-01, TC-M01-03
**Steps**:
  1. Install and launch user app (ignite_pay_app) on Android device or emulator
  2. Verify: Welcome screen displayed with "Create Identity" button
  3. Tap "Create Identity"
  4. Verify: Loading spinner appears, then DID is generated
  5. Verify: DID displayed in format `did:ignite:z6Mk...` (32+ chars)
  6. Verify: Mediator configuration screen appears
  7. Enter mediator URL (default or custom)
  8. Verify: "Connected to mediator" status shown (TC-M01-04)
  9. Verify: Dashboard displayed with DID identity
  10. Kill and relaunch app
  11. Verify: Wizard is skipped, dashboard loads directly (TC-M01-02)

**Pass Criteria**:
  - [ ] DID generated in correct format
  - [ ] Mediator connection established
  - [ ] DID persists across relaunch

**If fails**: Check mediator service is running (`make health`), check network connectivity

### T1.2 Merchant App — First Launch & DID Creation

**Flow**: Prerequisite for F17
**Business Scenario**: Event 1, Use Case 1.2
**Steps**:
  1. Install and launch merchant app (ignite_pay_merchant_app) on Android device
  2. Complete merchant onboarding flow
  3. Verify: <!-- State Channel: Exploration phase, not enabled - originally "Dual DID generated (state channel DID + DIDComm communication DID)" -->DID generated (DIDComm communication DID)
  4. Verify: Merchant ID displayed
  5. Configure merchant MCP connection

**Pass Criteria**:
  - [ ] Merchant DID created successfully
  - [ ] Merchant MCP connection configured

**If fails**: Check merchant MCP service, DID Registry service (:8081)

### T1.3 Phone <-> MCP Pairing (DIDComm Handshake)

**Flow**: F1
**App Test Case**: TC-M03-01, TC-M03-02, TC-M03-05
**Steps**:
  1. On MCP server, generate pairing QR code
  2. In user app, tap "Scan QR" or "Pair MCP"
  3. Scan the MCP pairing QR code
  4. Verify: DIDComm 3-step handshake initiates:
     - connection-request sent from phone
     - connection-response received from MCP
     - connection-confirm sent from phone
     - connection-confirm-response received from MCP
  5. Verify: SnackBar shows "Connected to MCP: did:ignite:..." (TC-M03-01)
  6. Verify: MCP status on dashboard shows "Connected"

**Error cases to verify**:
  7. Scan an invalid QR code (random QR) → verify error message (TC-M03-02)
  8. Attempt scan while not connected to mediator → verify appropriate message (TC-M03-05)

**Pass Criteria**:
  - [ ] Pairing completes with valid QR
  - [ ] Invalid QR rejected with clear error
  - [ ] Connection status shown on dashboard

**If fails**: Check DIDComm Router (:8080), mediator WebSocket connection

### T1.4 Merchant App <-> Merchant MCP Pairing

**Flow**: F1 (merchant variant)
**Business Scenario**: Event 2, Use Case 2.2
**Steps**:
  1. On merchant MCP server, generate pairing QR code
  2. In merchant app, scan the merchant MCP QR code
  3. Verify: DIDComm handshake completes using DIDComm communication DID
  4. Verify: Merchant app shows "Connected to Merchant MCP"

**Pass Criteria**:
  - [ ] Merchant pairing completes
  - [ ] Merchant MCP connection active

**If fails**: Check merchant DIDComm Router (:4000), merchant MCP logs

---

## Phase 2: Payment Authorization & Execution (Core)

### T2.1 x402 Challenge — First Payment, Session Key Creation

**Flow**: F2 + F4
**App Test Case**: TC-M04-01
**Prerequisites**: T1.3 completed (phone paired with MCP)

**Steps**:
  1. Start e-commerce demo: `cd ignite-pay-ecom-demo && python server.py`
  2. Agent sends: `GET /products`
  3. Verify: Product list returned with prices in lamports
  4. Agent sends: `POST /orders {"product_id": "coffee"}`
  5. Verify: HTTP 402 response with x402 `PaymentRequirements` in `PAYMENT-REQUIRED` header
  6. Verify: Response headers include `x402-merchant-did`, `x402-payment-address`, `x402-order-id`
  7. MCP processes challenge → parses PaymentRequirements → performs risk check
  8. MCP sends `payment-auth-request` to phone via DIDComm (mediator)
  9. Verify: Phone shows payment authorization screen with payment details
  10. Verify: Screen shows available payment methods
  11. User approves payment (slide to confirm)
  12. Phone creates ephemeral session key + registers on-chain
  13. Verify: Session key registered (check Solana devnet explorer)
  14. Fund session key if needed: `python fund_session.py <session_key_pubkey>`
  15. Phone sends `payment-auth-response` to MCP with session key
  16. MCP executes SOL transfer via session key on-chain
  17. Verify: Payment confirmed on Solana devnet (tx signature)
  18. Agent resends: `POST /orders {"product_id": "coffee"}` with `X-Payment-Proof` header
  19. Verify: HTTP 200, order status = "paid"

**Pass Criteria**:
  - [ ] 402 challenge received with correct format
  - [ ] Phone shows authorization request
  - [ ] Session key registered on-chain
  - [ ] Payment executed successfully
  - [ ] Order confirmed as paid (status transition pending_payment → paid)

**If fails**: Check MCP logs (`make logs S=ignite-pay-mcp`), phone mediator connection, devnet RPC, session key funding

### T2.2 Session Key — Subsequent Payment (Reuse)

**Flow**: F5
**App Test Case**: TC-M04-02
**Prerequisites**: T2.1 completed (active session key exists)

**Steps**:
  1. Agent sends another `POST /orders {"product_id": "tea"}`
  2. MCP receives 402 challenge
  3. Verify: MCP detects existing active session key, does NOT request phone authorization again
  4. MCP executes payment directly via existing session key
  5. Verify: Payment confirmed on Solana devnet
  6. Verify: Order status = "paid"

**Pass Criteria**:
  - [ ] Existing session key reused (no new phone authorization)
  - [ ] Payment executed successfully

**If fails**: Check session key not expired, check spending limit not exhausted

### T2.3 Session Key — Insufficient Balance Top-up

**Flow**: F3 + F7
**Prerequisites**: T2.1 completed, session key has low balance

**Steps**:
  1. Drain session key balance (send multiple payments until near zero)
  2. Agent sends `POST /orders {"product_id": "premium_coffee"}`
  3. MCP detects insufficient balance
  4. MCP sends `session-fund-request` to phone via DIDComm
  5. Verify: Phone shows top-up request screen
  6. User approves top-up
  7. Phone tops up session key on-chain
  8. Verify: Session key balance increased
  9. MCP executes payment
  10. Verify: Payment confirmed

**Pass Criteria**:
  - [ ] Low balance detected correctly
  - [ ] Top-up request sent to phone
  - [ ] Session key funded and payment proceeds

**If fails**: Check devnet faucet availability, session key validity

### T2.4 Session Key — Renewal / Replacement

**Flow**: F14
**App Test Case**: TC-M05-02, TC-M05-03, TC-M05-06
**Prerequisites**: T2.1 completed, session key near expiry

**Steps**:
  1. Wait for session key to approach expiration (or mock time)
  2. MCP detects expiring key, creates new ephemeral keypair
  3. MCP sends renewal request to phone
  4. Phone funds new session key
  5. Verify: New session key registered on-chain (TC-M05-02)
  6. Verify: New key card appears in app with "active" green badge
  7. Revoke old session key on-chain (TC-M05-03)
  8. Verify: Green SnackBar "Revoked on-chain: <tx_sig>"
  9. Verify: Old key shows "expired" status (TC-M05-06)

**Pass Criteria**:
  - [ ] New session key created and registered
  - [ ] Old key revoked on-chain
  - [ ] Subsequent payments use new key

**If fails**: Check key expiration logic, on-chain revocation transaction

---

## Phase 3: Merchant & QR Payments

### T3.1 QR Code — Merchant Generates QR

**Flow**: F17 prerequisite
**Business Scenario**: Event 8, Use Case 8.1
**Prerequisites**: T1.2 + T1.4 completed

**Steps**:
  1. In merchant app, tap "Generate Payment QR"
  2. Enter amount (e.g., 5.00 USDC)
  3. Verify: QR code displayed on screen
  4. Verify: QR code contains payment details (merchant DID, amount, order ID)
  5. Verify: Merchant app enters dual-channel wait (WebSocket + FCM)

**Pass Criteria**:
  - [ ] QR code generated with correct payment details
  - [ ] Merchant app waiting for payment

**If fails**: Check merchant MCP connection, QR generation service

### T3.2 QR Payment — User Scans and Pays

**Flow**: F17
**Business Scenario**: Event 8, Use Case 8.2
**Prerequisites**: T1.3 + T3.1 completed

**Steps**:
  1. In user app, tap "Scan QR" (payment scanner, not pairing scanner)
  2. Scan merchant QR code
  3. Verify: Payment details shown (merchant name, amount, order ID)
  4. Verify: Available payment methods displayed (session key, MagicBlock, relayer)
  5. User selects payment method (e.g., session key)
  6. User confirms payment (slide to confirm)
  7. App sends `qr-payment-request` to MCP via DIDComm
  8. MCP executes payment on-chain
  9. MCP sends `qr-payment-response` back to phone
  10. Verify: User app shows "Payment Successful"

**Pass Criteria**:
  - [ ] QR scanned and parsed correctly
  - [ ] Payment method selection works
  - [ ] Payment executed and confirmed
  - [ ] Success screen shown

**If fails**: Check QR format, DIDComm message routing, on-chain payment execution

### T3.3 Voice Announcement — Merchant Receives Notification

**Flow**: F18
**Business Scenario**: Event 8, Use Case 8.3
**Prerequisites**: T3.2 completed

**Steps**:
  1. After T3.2 payment success, buyer MCP sends `qr-payment-notify` to merchant MCP
  2. Merchant MCP receives notification
  3. Merchant MCP forwards to merchant app
  4. Verify: Merchant app plays voice announcement (e.g., "Payment received 5.00 USDC")
  5. Verify: Merchant app shows payment confirmation with tx signature

**Pass Criteria**:
  - [ ] Payment notification reaches merchant app
  - [ ] Voice announcement plays correctly
  - [ ] Payment details match original order

**If fails**: Check merchant MCP logs, push notification channel (FCM/WebSocket), TTS engine

---

## Phase 4: Risk Control & Merchant Management

### T4.1 Whitelist — Add Merchant, Verify Auto-Approve

**Flow**: F9
**App Test Case**: TC-M04-05
**Prerequisites**: T1.3 completed

**Steps**:
  1. Receive a payment-auth-request from a new merchant on phone
  2. On the authorization screen, tap "Add to Whitelist" action (TC-M04-05)
  3. Verify: Merchant added to whitelist
  4. Trigger another payment from the same merchant (below auto-approve threshold)
  5. Verify: MCP auto-approves the payment (no phone notification)
  6. Verify: Payment executes directly

**Pass Criteria**:
  - [ ] Whitelist addition works
  - [ ] Subsequent payments auto-approved within threshold

**If fails**: Check risk_check() logic, whitelist storage

### T4.2 Blacklist — Block Merchant, Verify Rejection

**Flow**: F9
**App Test Case**: TC-M04-06
**Prerequisites**: T1.3 completed

**Steps**:
  1. Receive a payment-auth-request from a merchant on phone
  2. On the authorization screen, tap "Add to Blacklist" action (TC-M04-06)
  3. Verify: Merchant added to blacklist
  4. Trigger another payment from the same merchant
  5. Verify: Payment automatically rejected (no phone notification needed)

**Pass Criteria**:
  - [ ] Blacklist addition works
  - [ ] Subsequent payments auto-rejected

**If fails**: Check blacklist lookup in risk_check()

### T4.3 New Merchant Authorization — First-Time Merchant Auth

**Flow**: F8
**Business Scenario**: Event 4, Use Case 4.2
**Prerequisites**: T1.3 completed, merchant NOT in whitelist

**Steps**:
  1. Agent sends payment request for a new (never-seen-before) merchant
  2. MCP detects merchant not in whitelist → triggers F8 flow
  3. MCP sends `payment-auth-request` to phone with additional merchant info
  4. Verify: Phone shows authorization with "New Merchant" indicator
  5. Verify: Screen shows merchant details and payment amount
  6. User approves
  7. Verify: Payment executes and merchant is recorded

**Pass Criteria**:
  - [ ] New merchant detected correctly
  - [ ] Phone shows new merchant authorization flow
  - [ ] Payment completes after authorization

**If fails**: Check merchant registry, risk evaluation logic

### T4.4 Authorization Exceeded — Limit Increase Request

**Flow**: F8 (limit exceeded variant)
**Prerequisites**: T1.3 completed, existing merchant with exhausted spending limit

**Steps**:
  1. Make payments to a merchant until spending limit is reached
  2. Trigger one more payment to the same merchant
  3. MCP detects spending limit exceeded → sends re-authorization request to phone
  4. Verify: Phone shows "Authorization Exceeded" with option to increase limit
  5. User increases limit and approves
  6. Verify: Payment executes with new limit

**Pass Criteria**:
  - [ ] Spending limit exceeded detected
  - [ ] Re-authorization flow triggered
  - [ ] Limit increase applied and payment succeeds

**If fails**: Check spending limit tracking, limit update persistence

---

## Phase 5: MagicBlock & Advanced Payment

### T5.1 MagicBlock Deposit — Deposit to Global Vault

**Flow**: F10
**Prerequisites**: T1.3 completed, MagicBlock configured

**Steps**:
  1. In user app, select "Deposit" or trigger via MCP
  2. Enter deposit amount (e.g., 1 SOL)
  3. MCP calls deposit to global buyer vault
  4. Verify: Deposit transaction on Solana devnet
  5. Verify: User's vault balance updated

**Pass Criteria**:
  - [ ] Deposit transaction confirmed on-chain
  - [ ] Vault balance reflects deposit

**If fails**: Check global vault PDA, deposit instruction, devnet RPC

### T5.2 MagicBlock Voucher Payment — Off-chain Voucher

**Flow**: F6
**Prerequisites**: T5.1 completed (funds in vault), merchant has MagicBlock channel

**Steps**:
  1. Agent sends payment request for MagicBlock-enabled merchant
  2. MCP selects voucher payment method
  3. MCP signs off-chain voucher with incremented sequence
  4. Verify: Voucher stored in VoucherStore
  5. Verify: Payment recorded off-chain

**Pass Criteria**:
  - [ ] Voucher signed with correct sequence
  - [ ] Payment recorded in off-chain store

**If fails**: Check voucher sequence allocation, MagicBlock channel state

### T5.3 MagicBlock Batch Settlement

**Flow**: F11
**Prerequisites**: T5.2 completed (accumulated vouchers)

**Steps**:
  1. Trigger batch settlement process
  2. MCP rebuilds Merkle trees from accumulated vouchers
  3. MCP signs batch settlement
  4. Merchant submits `settle_batch` or `optimistic_settle` on-chain
  5. Verify: Settlement transaction on Solana devnet
  6. Verify: Funds transferred to merchant

**Pass Criteria**:
  - [ ] Merkle tree rebuilt correctly
  - [ ] Settlement transaction confirmed
  - [ ] Merchant receives funds

**If fails**: Check Merkle root computation, settlement instruction, on-chain state

### T5.4 Dispute & Arbitration

**Flow**: F12
**Prerequisites**: T5.2 completed (disputed voucher)

**Steps**:
  1. Merchant disputes a voucher payment
  2. MCP provides Merkle proof for disputed payment
  3. Submit dispute resolution on-chain
  4. Verify: Dispute recorded on-chain
  5. If resolved in buyer's favor: verify `force_release` executed
  6. If resolved in merchant's favor: verify funds released to merchant

**Pass Criteria**:
  - [ ] Dispute submitted with valid Merkle proof
  - [ ] Resolution executed on-chain

**If fails**: Check Merkle proof validity, on-chain dispute instructions

### T5.5 Payment Method Selection — Choose Between Methods

**Flow**: F16
**Prerequisites**: T1.3 completed, multiple payment methods available

**Steps**:
  1. Receive payment request where multiple methods are available (session key, MagicBlock, relayer)
  2. MCP determines available methods and sends to phone
  3. Verify: Phone displays payment method selection screen
  4. Select each method and verify it works:
     - Session key: on-chain SOL/SPL transfer (F5)
     - MagicBlock: off-chain voucher (F6)
     - Relayer: sponsored payment (F16 variant)

**Pass Criteria**:
  - [ ] All available methods shown
  - [ ] Each method executes correctly when selected

**If fails**: Check method availability logic, individual method execution paths

### T5.6 Relayer (Sponsored) Payment

**Flow**: F16 variant
**Prerequisites**: T1.3 completed, relayer configured

**Steps**:
  1. Configure payment mode to "Sponsored" (TC-M08-04)
  2. Trigger a payment request
  3. MCP creates session key in sponsored mode
  4. Relayer sponsors the on-chain transaction (gas fees)
  5. Verify: Payment executes without user paying gas
  6. Verify: Transaction confirmed on Solana devnet

**Pass Criteria**:
  - [ ] Sponsored session key created
  - [ ] Payment executes without user gas fees
  - [ ] Transaction confirmed

**If fails**: Check relayer service, sponsored session key registration

---

<!-- State Channel: Exploration phase, not enabled
## Phase 6: State Channel Operations

### T6.1 Open Channel

**State Channel Scenario**: SC-01
**Prerequisites**: Channel services running (`make health`), keypairs in `deploy/keys/`

**Steps**:
  1. User selects a Hub for channel creation
  2. POST `/v1/channels/open` with channel parameters (user, provider, deposit, tree_depth)
  3. Verify: Channel PDA created on Solana devnet
  4. Verify: Initial deposit locked in escrow PDA
  5. POST `/v1/channels/fund` with additional funding
  6. Verify: Escrow balance increased
  7. POST `/v1/channels/split` to initialize balance split
  8. Verify: Merkle tree initialized with correct leaf balances
  9. Verify: Amount conservation (leaf amounts sum = total deposited)

**Pass Criteria**:
  - [ ] Channel account created on-chain (status = Open)
  - [ ] Escrow funded correctly
  - [ ] Merkle root matches leaf hashes
  - [ ] Balance conservation holds

**If fails**: Check keypair files in `deploy/keys/`, Solana devnet connectivity, channel service logs

### T6.2 Off-chain Payment

**State Channel Scenario**: SC-02
**Prerequisites**: T6.1 completed (channel open)

**Steps**:
  1. POST `/v1/channels/{id}/pay` with payment amount and recipient
  2. Service builds LeafUpdate with new balance allocation
  3. Verify: `sign_leaf_update` produces valid Ed25519 signature
  4. POST `/v1/channels/{id}/cosign` for provider co-signature
  5. Verify: `apply_leaf_update` applied correctly
  6. Verify: Merkle root updated
  7. Verify: Sequence number incremented consecutively
  8. Repeat with multiple payments
  9. Verify: All state updates are consistent

**Pass Criteria**:
  - [ ] Leaf update signed correctly
  - [ ] Provider co-signs
  - [ ] Merkle root updates with each payment
  - [ ] Sequence numbers are consecutive

**If fails**: Check signature verification, state consistency

### T6.3 Batch Pipeline Payment

**State Channel Scenario**: SC-03
**Prerequisites**: T6.1 completed

**Steps**:
  1. POST `/v1/channels/{id}/batch` with multiple operations
  2. Pipeline created with `Pipeline::new()`
  3. Add operations: `transfer_leaf`, `partial_transfer`, `create_htlc`
  4. Execute pipeline: `build()` applies all operations
  5. Verify: All operations succeed atomically (all or nothing)
  6. Verify: Merkle root reflects all changes
  7. Test rollback: submit invalid operation → `abort()` called
  8. Verify: State unchanged after abort

**Pass Criteria**:
  - [ ] Batch executes atomically
  - [ ] All leaf updates applied correctly
  - [ ] Rollback works on failure

**If fails**: Check pipeline atomicity, state snapshot/restore logic

### T6.4 HTLC Conditional Payment

**State Channel Scenario**: SC-04
**Prerequisites**: T6.1 completed

**Steps**:
  1. POST `/v1/channels/{id}/htlc/create` with hash_lock and timelock
  2. Verify: HTLC created with correct parameters
  3. Verify: Timelock > current_slot + challenge_duration + 1000 (HOP_MARGIN)
  4. Reveal preimage: POST `/v1/channels/{id}/htlc/resolve` with preimage
  5. Verify: Preimage matches hash_lock (SHA-256)
  6. Verify: HTLC resolved, funds transferred
  7. Test refund: create HTLC, wait for timelock expiry
  8. POST `/v1/channels/{id}/htlc/refund`
  9. Verify: Funds returned to original owner

**Pass Criteria**:
  - [ ] HTLC created with valid constraints
  - [ ] Preimage resolves correctly
  - [ ] Timelock refund works after expiry

**If fails**: Check hash computation, timelock slot values, preimage verification

### T6.5 Cooperative Close

**State Channel Scenario**: SC-05
**Prerequisites**: T6.1 + T6.2 completed (channel with off-chain state)

**Steps**:
  1. Verify: No active HTLCs exist in channel
  2. POST `/{id}/close` with cooperative settlement terms
  3. Both parties sign `cooperative_settle` instruction
  4. Verify: Channel status → Settling
  5. POST `/{id}/claim` with Merkle proof for each leaf
  6. Verify: Funds claimed by respective owners
  7. POST `/{id}/finalize` to complete settlement
  8. Verify: Channel closed, all funds distributed

**Pass Criteria**:
  - [ ] Dual signatures obtained
  - [ ] Merkle proofs valid for all claims
  - [ ] Funds distributed correctly
  - [ ] Channel status = Finalized

**If fails**: Check for active HTLCs, signature validity, Merkle proof computation

### T6.6 Dispute Resolution

**State Channel Scenario**: SC-06
**Prerequisites**: T6.1 + T6.2 completed, disagreement scenario

**Steps**:
  1. POST `/{id}/challenge` — trigger challenge with signed state
  2. Verify: Channel status → Challenged
  3. Counterparty submits: POST `/{id}/submit-counter` with higher-sequence state
  4. Verify: Both sig_a + sig_b validated
  5. Verify: Counter-state has higher sequence number
  6. Wait for challenge duration to elapse
  7. POST `/{id}/settle` — settle after timeout
  8. Verify: Channel status → Settling with last valid state

**Pass Criteria**:
  - [ ] Challenge triggered successfully
  - [ ] Counter-state accepted (higher sequence)
  - [ ] Settlement after timeout uses correct state

**If fails**: Check challenge duration, sequence numbers, signature pairs

### T6.7 Hub Routing

**State Channel Scenario**: SC-08
**Prerequisites**: Multiple hubs registered

**Steps**:
  1. POST `/v1/hub/register` — register a new hub
  2. POST `/hub/metrics` — update hub metrics (latency, reliability)
  3. POST `/routes/add-edge` — add routing edges between hubs
  4. POST `/routes/find` — discover route from user to merchant
  5. Verify: RouteService returns best route based on metrics
  6. Test with no available route → verify graceful failure

**Pass Criteria**:
  - [ ] Hub registration works
  - [ ] Route discovery finds valid paths
  - [ ] Best route selected based on metrics

**If fails**: Check hub registry, route graph connectivity

---

## Phase 7: E2E Demo

### T7.1 Start E-commerce Demo Server

**Prerequisites**: All backend services running, session key funded

**Steps**:
  1. `cd ignite-pay-ecom-demo`
  2. Install dependencies: `pip install -r requirements.txt`
  3. Start server: `python server.py`
  4. Verify: Server listening on port 9090
  5. Test: `curl http://localhost:9090/products`
  6. Verify: JSON array of products returned

**Pass Criteria**:
  - [ ] Server starts without errors
  - [ ] Products endpoint returns valid JSON

**If fails**: Check Python dependencies, port availability

### T7.2 Run Mock Test

**Steps**:
  1. Run mock test script: `python test_flow.py` (if available)
  2. Verify: All mock payment flows complete
  3. Verify: No errors in output

**Pass Criteria**:
  - [ ] Mock test passes end-to-end

**If fails**: Check mock configuration, service connectivity

### T7.3 Full E2E: Agent -> x402 -> MCP -> Phone -> Payment -> Order

**Prerequisites**: T7.1 completed, phone paired with MCP (T1.3)

**Steps**:
  1. Agent calls `GET /products` → receives product list
  2. Agent calls `POST /orders {"product_id": "coffee"}` → receives 402 challenge
  3. Agent calls MCP `process_x402_challenge()` → MCP sends to phone
  4. Phone approves → session key created → registered on-chain
  5. MCP executes payment → Solana confirms
  6. Agent resends `POST /orders` with `X-Payment-Proof`
  7. Server verifies on-chain transaction:
     - tx exists and confirmed
     - no error in tx
     - balance increase >= expected at recipient
  8. Verify: Order status = "paid" with tx_signature

**Pass Criteria**:
  - [ ] Complete flow from product listing to paid order
  - [ ] On-chain verification passes
  - [ ] Order confirmed with valid transaction signature

**If fails**: Trace each step — check MCP logs, phone mediator, devnet RPC, payment proof format

---

## Phase 8: App Settings & Edge Cases

### T8.1 Network Switching

**App Test Case**: TC-M08-01, TC-M08-02
**Steps**:
  1. Go to Settings → Network
  2. Switch from devnet to mainnet (TC-M08-01)
  3. Verify: RPC URL updates, app reconnects
  4. Set custom RPC URL (TC-M08-02)
  5. Kill and relaunch app
  6. Verify: Custom RPC URL persists

### T8.2 Payment Mode Switching

**App Test Case**: TC-M08-04
**Steps**:
  1. Go to Settings → Payment Mode
  2. Switch from "Self-Funded" to "Sponsored"
  3. Trigger a payment
  4. Verify: Payment uses sponsored (relayer) mode
  5. Switch back to "Self-Funded"
  6. Verify: Next payment uses self-funded session key

### T8.3 Deep Link Callback

**App Test Case**: TC-M10-01, TC-M10-02, TC-M10-03
**Prerequisites**: External wallet (Phantom/Solflare) installed

**Steps**:
  1. Pair MCP, receive payment request
  2. Select Phantom/Solflare as signing method
  3. Wallet opens, user signs
  4. Deep link callback: `ignitepay://onchain?signature=...` (TC-M10-01)
  5. Verify: App receives callback, key registered
  6. Test: Callback with no pending transaction (TC-M10-02) → verify ignored
  7. Test: Callback with invalid signature (TC-M10-03) → verify error shown

### T8.4 Message List & Filters

**App Test Case**: TC-M06-01 through TC-M06-06
**Steps**:
  1. Open Messages screen (TC-M06-01)
  2. Verify: Message list shows recent messages
  3. Filter by type: All / Payment / List Sync / Connection (TC-M06-02)
  4. Tap payment message → verify ChallengeScreen opens (TC-M06-03)
  5. Tap non-payment message → verify detail dialog (TC-M06-04)
  6. Pull to refresh → verify list updates (TC-M06-05)
  7. Clear all messages → verify empty state (TC-M06-06)

### T8.5 Risk Control Policies

**App Test Case**: TC-M07-01, TC-M07-02, TC-M07-03
**Steps**:
  1. Open Risk Control screen (TC-M07-01)
  2. Verify: Policy list shows 4 merchant cards
  3. Tap a card to expand details (TC-M07-02)
  4. Toggle auto-pay on/off (TC-M07-03)
  5. Verify: Toggle persists after navigation

---

## Test Results Tracking

Copy this table and fill in during testing:

| Test ID | Test Name | Result | Notes | Date |
|---------|-----------|--------|-------|------|
| T1.1 | User App First Launch | | | |
| T1.2 | Merchant App First Launch | | | |
| T1.3 | Phone-MCP Pairing | | | |
| T1.4 | Merchant-MCP Pairing | | | |
| T2.1 | x402 First Payment | | | |
| T2.2 | Session Key Reuse | | | |
| T2.3 | Insufficient Balance Top-up | | | |
| T2.4 | Session Key Renewal | | | |
| T3.1 | QR Code Generation | | | |
| T3.2 | QR Payment Scan | | | |
| T3.3 | Voice Announcement | | | |
| T4.1 | Whitelist Auto-Approve | | | |
| T4.2 | Blacklist Rejection | | | |
| T4.3 | New Merchant Authorization | | | |
| T4.4 | Authorization Exceeded | | | |
| T5.1 | MagicBlock Deposit | | | |
| T5.2 | Voucher Payment | | | |
| T5.3 | Batch Settlement | | | |
| T5.4 | Dispute & Arbitration | | | |
| T5.5 | Payment Method Selection | | | |
| T5.6 | Relayer Payment | | | |
<!-- State Channel: Exploration phase, not enabled
| T6.1 | Open Channel | | | |
| T6.2 | Off-chain Payment | | | |
| T6.3 | Batch Pipeline | | | |
| T6.4 | HTLC Payment | | | |
| T6.5 | Cooperative Close | | | |
| T6.6 | Dispute Resolution | | | |
| T6.7 | Hub Routing | | | |
-->
| T7.1 | E-commerce Demo Start | | | |
| T7.2 | Mock Test | | | |
| T7.3 | Full E2E Flow | | | |
| T8.1 | Network Switching | | | |
| T8.2 | Payment Mode Switching | | | |
| T8.3 | Deep Link Callback | | | |
| T8.4 | Message List & Filters | | | |
| T8.5 | Risk Control Policies | | | |

**Result values**: PASS | FAIL | SKIP | N/A
