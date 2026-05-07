# Ignite Pay — Business Flows Overview

## 1. Flow Inventory

| # | Flow Name | Status | Participants |
|---|-----------|--------|-------------|
| F1 | Phone Pairing (DIDComm 3-step handshake) | ✅ Implemented | MCP ↔ Phone |
| F2 | Session Key Creation (embedded in payment flow; MCP creates on demand + phone registers & funds authorization in one step) | ✅ Implemented | MCP → Phone → Solana |
| F3 | Session Key Top-up (request top-up when balance is insufficient) | ✅ Implemented | MCP ↔ Phone → Solana |
| F4 | x402 Payment Authorization (including payment method selection) | ✅ Implemented | Agent → MCP ↔ Phone |
| F5 | Payment Execution (Session Key on-chain transfer) | ✅ Implemented | MCP → Solana |
| F6 | Payment Execution (MagicBlock Voucher) | ✅ Implemented | MCP → VoucherStore |
| F7 | Insufficient Balance → Top-up Request | ✅ Implemented | MCP ↔ Phone |
| F8 | Merchant Unauthorized / Authorization Exceeded → Additional Authorization | ✅ Implemented | MCP ↔ Phone |
| F9 | Merchant Whitelist/Blacklist Management | ✅ Implemented | MCP / Phone → MCP |
| F10 | MagicBlock Global Vault Deposit | ✅ Implemented | MCP → Solana |
| F11 | MagicBlock Batch Settlement | ✅ Implemented | MCP / Merchant → Solana |
| F12 | Dispute & Arbitration | ✅ Implemented | MCP / Merchant → Solana |
| F13 | Balance Query & Notification | ✅ Implemented | MCP ↔ Phone |
| F14 | Session Key Renewal / Replacement | ✅ Implemented | MCP ↔ Phone |
| F15 | Multi-Merchant Concurrent Payments | ✅ Implemented | Agent → MCP → Solana |
| F16 | Payment Method Selection (Session Key / MagicBlock / Relayer) | ✅ Implemented | MCP ↔ Phone |
| F17 | User Scans Merchant QR Code Payment (QR → Phone → MCP → Execute Payment) | ✅ Implemented | Phone → MCP → Solana |
| F18 | Merchant Voice Announcement (Notify merchant MCP after QR payment success → Merchant App announcement) | ✅ Implemented | Buyer MCP → Merchant MCP → Merchant App |

---

## 2. Detailed Flow Descriptions

### F1: Phone Pairing (DIDComm 3-step handshake)

**Status**: ✅ Implemented

The user scans a QR code to pair. MCP and the phone establish a DIDComm encrypted channel. All subsequent messages are transmitted encrypted through this channel.

```
Phone                           MCP                           Mediator
  |                               |                               |
  | scan QR (OOB invitation)      |                               |
  |                               |                               |
  |--- connection-request --------|------------------------------>|
  |                               |                               |
  |<-- connection-response -------|<------------------------------|
  |   (MCP signs nonce)           |                               |
  |                               |                               |
  |--- connection-confirm ------->|------------------------------>|
  |   (Phone signs nonce)         |                               |
  |                               |                               |
  |<-- connection-confirm-resp ---|<------------------------------|
  |   (mutual signature verification complete)                    |
```

**Code Locations**:
- MCP-side handling: `ignite-pay-mcp/src/mediator.rs:858-1235`
- DIDComm message construction: `ignite-pay-core/src/didcomm.rs` — `build_connection_request`, `build_connection_response`, `build_connection_confirm`, `build_connection_confirm_response`
- Phone-side: `ignite_pay_app/rust/src/api/simple.rs:549` — `send_connection_request`

**DIDComm Message Types**:
- `ignite-pay/1.0/connection-request`
- `ignite-pay/1.0/connection-response`
- `ignite-pay/1.0/connection-confirm`
- `ignite-pay/1.0/connection-confirm-response`

---

### F2: Session Key Creation (Embedded in Payment Flow)

**Status**: ✅ Implemented

Session keys **can only be created locally by MCP** (MCP exclusively holds the private key), but MCP does not proactively/pre-emptively create them. **Only when a payment is needed and no session key is available**, MCP generates an ephemeral keypair locally, then **sends the session key information along with the payment authorization request** to the phone via DIDComm. The phone **simultaneously handles** three things: on-chain session key account registration, top-up (SOL gas + stablecoins), and user payment authorization.

**Complete Target Flow**:

```
MCP                              Phone                          Solana
  |                                |                               |
  | [x402 payment request arrives] |                               |
  | Check: no available session key|                               |
  |                                |                               |
  | 1. Generate ephemeral keypair locally                          |
  |    (MCP holds private key exclusively)                         |
  |                                |                               |
  | 2. DIDComm: payment-auth-req ->|                               |
  |   + payment info:              |                               |
  |     merchant_did, amount, ...  |                               |
  |   + session key info:          |                               |
  |     ephemeral_pubkey           |                               |
  |     spending_limit             |                               |
  |     suggested top-up amount    |                               |
  |                                |                               |
  |                                | 3. Phone simultaneously handles:|
  |                                |                                |
  |                                | 3a. On-chain session key registration
  |                                |--- register_session ---------->|
  |                                |    (owner + ephemeral signature)|
  |                                |<-- tx sig ---------------------|
  |                                |                                |
  |                                | 3b. Top-up                     |
  |                                |--- transfer SOL ------------->|
  |                                |    (owner → ephemeral)         |
  |                                |--- transfer USDC ------------>|
  |                                |    (owner_ATA → ephemeral_ATA) |
  |                                |                                |
  |                                | 3c. User authorizes payment    |
  |                                |    (display payment details, user confirms)
  |                                |                                |
  |<-- payment-auth-response ------|                               |
  |   + authorized: true           |                               |
  |   + session_key_pubkey         |                               |
  |   + session_key_tx_sig (registration)                          |
  |   + top-up tx sigs             |                               |
  |   + spending_limit, expires_at |                               |
  |                                |                               |
  | 4. MCP saves session key locally                               |
  |    (keypair already exists, record authorization info)         |
  |                                |                               |
  | 5. execute_payment ----------------------------------------->|
  |    (signed with session key)   |                               |
```

**Current Implementation vs. Target Gap**:

| Step | Status |
|------|--------|
| MCP auto-creates ephemeral keypair (in payment flow) | ✅ `process_x402_challenge` → `create_session_key_for_request` |
| payment-auth-request includes session key + secret key | ✅ `new_session_key` object contains pubkey, secret_key, spending_limit, suggested_funding |
| Phone parses new_session_key fields | ✅ `DecryptedMessage` extended + `decrypt_message()` parsing |
| Phone on-chain session key registration (external key) | ✅ `register_external_session_key()` |
| Phone top-up SOL + SPL token | ✅ `fund_session_key()` |
| Phone handles registration+top-up+authorization simultaneously | ✅ `register_and_fund_session_key()` + challenge_screen integration |
| payment-auth-response return | ✅ Session key data fields exist |

**Code Locations**:
- MCP session key creation: `ignite-pay-mcp/src/main.rs` — `create_session_key_for_request()`
- DIDComm message construction: `ignite-pay-core/src/didcomm.rs` — `build_authorization_request_inner()` + `NewSessionKeyRequest`
- Phone parsing: `ignite_pay_app/rust/src/api/simple.rs` — `decrypt_message()`
- Phone external key registration: `ignite_pay_app/rust/src/api/session.rs` — `register_external_session_key()`
- Phone top-up: `ignite_pay_app/rust/src/api/session.rs` — `fund_session_key()`
- Phone one-step completion: `ignite_pay_app/rust/src/api/session.rs` — `register_and_fund_session_key()`
- Flutter integration: `ignite_pay_app/lib/challenge_screen.dart` — `_onAuthorize()` MCP key path

---

### F3: Session Key Insufficient Balance → Top-up Request

**Status**: ✅ Implemented

When MCP has a session key but the balance is insufficient to complete a payment, it requests the phone user to top up via DIDComm.

**Target Flow**:

```
MCP                              Phone                          Solana
  |                                |                               |
  | Attempt payment, detect insufficient balance                  |
  |                                |                               |
  |--- session-fund-request (DC) ->|                               |
  |   "Session key balance insufficient"                           |
  |   "Current balance: X SOL"     |                               |
  |   "Required: Y SOL + Z USDC"   |                               |
  |   "Public key: <ephemeral_pubkey>"                             |
  |                                |                               |
  |                                | User choice:                  |
  |                                | A. Top up (enter amount)      |
  |                                | B. Reject                     |
  |                                |                               |
  |                                | [If option A selected]        |
  |                                |--- transfer SOL ------------->|
  |                                |    (owner → ephemeral)         |
  |                                |--- transfer USDC ------------>|
  |                                |    (owner_ATA → ephemeral_ATA) |
  |                                |                               |
  |<-- session-fund-response -----|                               |
  |   { action: "funded",          |                               |
  |     sol_tx: "...",             |                               |
  |     usdc_tx: "..." }           |                               |
  |   or                           |                               |
  |   { action: "rejected" }       |                               |
  |                                |                               |
  | [If top-up successful]         |                               |
  |--- execute_payment --------------------------------------->|
```

**Required DIDComm Message Types** (implemented):
- `ignite-pay/1.0/session-fund-request` — MCP → Phone, request top-up ✅
- `ignite-pay/1.0/session-fund-response` — Phone → MCP, confirm top-up ✅

**Required Code**:
- MCP: Send `session-fund-request` when insufficient balance is detected ✅
- Phone: Receive request, display top-up interface, execute on-chain transfer ✅
- Phone: Send `session-fund-response` ✅
- MCP: Confirm balance then continue payment ✅

---

### F4: x402 Payment Authorization (Including Payment Method Selection)

**Status**: ✅ Implemented

MCP parses the x402 challenge, verifies the merchant DID, makes a risk control decision:
- Auto-approve (whitelist/global threshold) → MCP directly executes payment (prefer MagicBlock, fallback to Session Key)
- Requires authorization → MCP determines available payment methods, sends `payment-auth-request` (with `available_payment_methods`), phone user selects a method and returns the `payment_method` field

`payment-auth-request` new fields:
```json
{
  "payment_id": "pay-123",
  "merchant_did": "did:ignite:zMerchant",
  "amount": 500000000,
  "description": "...",
  "available_payment_methods": ["session_key", "magicblock"],
  "new_session_key": { ... }
}
```

`payment-auth-response` new fields:
```json
{
  "payment_id": "pay-123",
  "authorized": true,
  "payment_method": "magicblock",
  "..."
}
```

**Code Locations**:
- `PaymentMethod` enum: `ignite-pay-core/src/didcomm.rs` — `PaymentMethod::SessionKey`, `PaymentMethod::MagicBlock`, `PaymentMethod::Relayer`
- `get_available_payment_methods()`: `ignite-pay-mcp/src/main.rs`
- `has_mb_channel()`: `ignite-pay-mcp/src/main.rs`
- `execute_payment_auto()`: accepts `preferred_method` parameter, executes based on user selection

Agent encounters paywall → MCP parses x402 → risk control decision → requests phone authorization via DIDComm when needed.

See `docs/agent-payment-flow.md` for detailed flow.

**DIDComm Message Types**:
- `ignite-pay/1.0/payment-auth-request` — MCP → Phone
- `ignite-pay/1.0/payment-auth-response` — Phone → MCP

---

### F5: Payment Execution (Session Key On-chain Transfer)

**Status**: ✅ Implemented

MCP signs an on-chain transaction using the session key, executing SOL/SPL transfer through the session key contract's CPI.

```
MCP                                   Solana
  |                                      |
  | build execute_payment IX             |
  | sign with session keypair            |
  |                                      |
  |--- sendTransaction ----------------->|
  |                                      |
  |   on-chain:                          |
  |     verify session valid             |
  |     verify not expired               |
  |     verify spending limit            |
  |     CPI: transfer(ephemeral→merchant)|
  |     update current_spent             |
  |                                      |
  |<-- tx signature ---------------------|
```

**Prerequisites**:
1. Session key is registered on-chain
2. Ephemeral address has sufficient SOL/stablecoins
3. Spending limit is not exhausted
4. Session has not expired

**Code Locations**:
- MCP: `ignite-pay-mcp/src/main.rs:216` — `execute_payment()`
- Solana: `ignite-pay-solana/src/sessionprogram.rs:78` — `build_execute_payment_ix()`
- On-chain program: `ignite-pay-session/programs/ignite-pay-session/src/lib.rs`

---

### F6: Payment Execution (MagicBlock Voucher)

**Status**: ✅ Implemented

If the merchant has a MagicBlock channel (spending cap accounting), MCP signs an off-chain voucher. Funds remain in the unified vault.

```
MCP                              VoucherStore
  |                                   |
  | derive channel PDA                |
  | query channel on-chain            |
  | check spending_cap - settled      |
  |                                   |
  | sign_voucher(channel_id, seq, $)  |
  | SHA256(channel_id ‖ seq ‖ amount) |
  | Ed25519 sign                      |
  |                                   |
  | store voucher -------------------->|
  |                                   |
  | return voucher proof to Agent     |
```

**Code Locations**:
- MCP: `ignite-pay-mcp/src/main.rs:294` — `try_mb_voucher_payment()`
- Signing: `ignite-pay-mb/sdk/src/signing.rs:33` — `sign_voucher()`
- Storage: `ignite-pay-mcp/src/voucher_store.rs`

---

### F7: Insufficient Balance → Top-up Request

**Status**: ✅ Implemented

When MCP attempts a payment and detects that the session key balance is insufficient (SOL or stablecoins), it needs to notify the phone user to top up.

**Required Flow**:

```
MCP                              Phone                          Solana
  |                                |                               |
  | Attempt execute_payment        |                               |
  | Detect balance < amount        |                               |
  |                                |                               |
  |--- fund-request (DIDComm) ---->|                               |
  |   "Insufficient balance, top-up required"                     |
  |   "Current: X SOL, Required: Y SOL"                           |
  |                                |                               |
  |                                | User choice:                  |
  |                                | A. Top up (enter amount)      |
  |                                | B. Reject                     |
  |                                |                               |
  |                                | [If option A selected]        |
  |                                |--- transfer(owner→ephemeral)->|
  |                                |                               |
  |<-- fund-response (DIDComm) ----|                               |
  |   { action: "funded", tx: .. } |                               |
  |   or                           |                               |
  |   { action: "rejected" }       |                               |
  |                                |                               |
  | [If top-up successful]         |                               |
  |--- execute_payment --------------------------------------->|
```

**Implemented Components**:
1. ✅ MCP balance detection logic (check ephemeral address balance before executing payment)
2. ✅ DIDComm message types `session-fund-request` / `session-fund-response`
3. ✅ Phone-side top-up interface + on-chain transfer
4. ✅ MCP wait for top-up response then retry payment

---

### F8: Merchant Unauthorized / Authorization Exceeded → Additional Authorization

**Status**: ✅ Implemented

When MCP needs to make a payment to a merchant, if the user has not explicitly authorized that merchant before (not in the whitelist), or if the cumulative payment amount to that merchant has exceeded the user-set authorization limit, it needs to request additional authorization from the phone user.

**Required Flow**:

```
Scenario A: Merchant not in whitelist (unauthorized)
MCP                              Phone
  |                                |
  | risk_check → NeedsAuth        |
  |                                |
  |--- merchant-auth-request ----->|
  |   "New merchant authorization request"                        |
  |   "Merchant DID: did:ignite:z..."                             |
  |   "Requested amount: X"       |
  |   "Authorization options: one-time/by limit/permanent"        |
  |                                |
  |                                | User choice:
  |                                | A. Authorize (set limit and duration)
  |                                | B. Reject
  |                                |
  |<-- merchant-auth-response -----|
  |   { authorized: true,          |
  |     max_amount: X,             |
  |     label: "trusted",          |
  |     duration: 86400 }          |
  |   or                           |
  |   { authorized: false }        |

Scenario B: Merchant authorized but limit insufficient (exceeded)
MCP                              Phone
  |                                |
  | Cumulative payments > whitelist.max_amount                    |
  |                                |
  |--- merchant-auth-request ----->|
  |   "Merchant limit nearly exhausted"                           |
  |   "Used: X, Limit: Y"         |
  |   "Amount needed this time: Z" |
  |                                |
  |                                | User choice:
  |                                | A. Increase limit
  |                                | B. Authorize this time only
  |                                | C. Reject
  |                                |
  |<-- merchant-auth-response -----|
```

**Comparison with Current Implementation**:
- Current `payment-auth-request` only handles single payment authorization, does not distinguish between "merchant authorization" and "payment authorization"
- Whitelist mechanism exists (`ListStore`), but there is no DIDComm message for "request additional authorization when merchant limit is exhausted"
- The `list_action` field in `payment-auth-response` can trigger whitelist updates, but this is a post-action (updated after payment)

**Implemented Components**:
1. ✅ Merchant authorization limit tracking (cumulative payments vs. authorization limit)
2. ✅ Automatic detection when limit is exhausted
3. ✅ Differentiated DIDComm messages (distinguish between "new merchant authorization" and "limit increase")
4. ✅ Phone-side merchant authorization management interface

---

### F9: Merchant Whitelist/Blacklist Management

**Status**: ✅ Implemented

MCP supports whitelist and blacklist management via tool calls. When the phone authorizes a payment, it can trigger automatic addition to the whitelist via the `list_action` field.

**Code Locations**:
- MCP tools: `add_merchant`, `update_merchant`, `remove_merchant`, `verify_merchant`
- Risk control: `ignite-pay-core/src/list_store.rs` — `risk_check()`
- Trigger: `process_x402_challenge` handling of `resp.list_action`

---

### F10: MagicBlock Global Vault Deposit

**Status**: ✅ Implemented

Users deposit SOL into the global vault through MCP's `mb_deposit` tool.

```
User → MCP.mb_deposit(amount) → Solana (deposit instruction)
                               → global_buyer_vault lamports += amount
                               → GlobalState.total_deposited += amount
```

**Code Locations**:
- MCP tool: `ignite-pay-mcp/src/main.rs:1344` — `mb_deposit`
- Transaction construction: `ignite-pay-mb/sdk/src/transaction.rs:143` — `build_deposit_tx()`
- On-chain processing: `ignite-pay-mb/programs/ignite-pay-mb/src/lib.rs:96` — `deposit` instruction

---

### F11: MagicBlock Batch Settlement

**Status**: ✅ Implemented (at MCP tool level)

MCP can rebuild Merkle trees and sign batch settlements. Merchants can submit `settle_batch` or `optimistic_settle` on-chain.

**Code Locations**:
- MCP: `mb_sign_settlement`, `mb_sign_voucher`
- SDK: `ignite-pay-mb/sdk/src/merkle.rs` — Sum Merkle tree
- On-chain: `ignite-pay-mb/programs/ignite-pay-mb/src/lib.rs:119` — `settle_batch`

---

### F12: Dispute & Arbitration

**Status**: ✅ Implemented (at on-chain program level)

Buyers can dispute a settlement, provide Merkle proof to resolve disputes, and merchants can force_release after the dispute_period.

**Code Locations**:
- MCP tools: `mb_dispute`, `mb_resolve_dispute`
- On-chain: `dispute`, `resolve_dispute`, `force_release` instructions

---

### F13: Balance Query & Notification

**Status**: ✅ Implemented

MCP has no mechanism to periodically query session key or global vault balances, nor does it proactively notify the phone of insufficient balances.

**Required Flow**:

```
MCP                              Phone
  |                                |
  | Periodically check:            |
  |   session key SOL balance      |
  |   session key USDC balance     |
  |   MagicBlock vault balance     |
  |   spending limit remaining     |
  |                                |
  | [When balance falls below threshold]                          |
  |--- balance-notification ------->|
  |   "SOL balance: 0.01 (low)"    |
  |   "USDC balance: 5.00"         |
  |   "Suggested top-up"           |
```

---

### F14: Session Key Renewal / Replacement

**Status**: ✅ Implemented

After a session key expires, a new one must be manually created. There is no automatic renewal or seamless replacement mechanism.

**Target Flow** (MCP creates new key → Phone tops up):

```
MCP                              Phone                          Solana
  |                                |                               |
  | Detect session about to expire |                               |
  |                                |                               |
  | Create new ephemeral keypair locally                           |
  | Optional: register new session PDA on-chain                   |
  |                                |                               |
  |--- session-renew-request ----->|                               |
  |   "New session key created"    |                               |
  |   "Public key: <new_ephemeral>"|                               |
  |   "Please top up: X SOL + Y USDC"                             |
  |                                |                               |
  |                                | User confirms top-up          |
  |                                |--- transfer(owner→new_key) --->|
  |                                |                               |
  |<-- session-renew-response -----|                               |
  |   { action: "funded",          |                               |
  |     sol_tx: "...",             |                               |
  |     usdc_tx: "..." }           |                               |
  |                                |                               |
  | Replace old session key        |                               |
  | Old session optional refund ---|------------------------------>|
```

---

### F15: Multi-Merchant Concurrent Payments

**Status**: ✅ Implemented

MCP's `process_x402_challenge` supports concurrent request processing. Through payment mutex and atomic execution mechanisms:

- ✅ When sharing the same session key, spending limit checks are guaranteed atomic via mutex
- ✅ MagicBlock voucher seq allocation has concurrency protection
- ✅ Payment queue and priority mechanisms are implemented

---

### F16: Payment Method Selection (Session Key / MagicBlock / Relayer)

**Status**: ✅ Implemented

Users can choose the payment method when authorizing on the phone. MCP determines available methods based on current state and sends them to the phone; the phone user selects a method and MCP executes accordingly.

**Supported Methods**:

| Method | Description | Status |
|--------|-------------|--------|
| `session_key` | Session Key on-chain direct transfer | ✅ Available |
| `magicblock` | MagicBlock off-chain voucher signing | ✅ Available (requires channel) |
| `relayer` | Sponsored payment mode + Relayer service | ✅ Available (session key signs, relayer pays gas) |

**Flow**:
```
MCP                                             Phone
  |                                                |
  | determine available_payment_methods            |
  |   - session_key: always available              |
  |   - magicblock: if channel exists on-chain     |
  |                                                |
  |--- payment-auth-request (available_methods) -->|
  |                                                |
  |                    User sees: [Session Key] [MagicBlock]
  |                    User selects method
  |                                                |
  |<-- payment-auth-response (payment_method) -----|
  |                                                |
  | execute_payment_auto(preferred_method)         |
  |   - "session_key" → on-chain transfer          |
  |   - "magicblock" → sign voucher                |
  |   - "relayer" → execute_payment_sponsored (session key signs, relayer pays gas) |
```

**Code Locations**:
- `PaymentMethod` enum: `ignite-pay-core/src/didcomm.rs:12-30`
- `get_available_payment_methods()`: `ignite-pay-mcp/src/main.rs`
- `has_mb_channel()`: `ignite-pay-mcp/src/main.rs`
- `execute_payment_auto()`: `ignite-pay-mcp/src/main.rs` — accepts `preferred_method` parameter
- `build_authorization_request_with_methods()`: `ignite-pay-core/src/didcomm.rs`
- `build_authorization_response_v1_3()`: `ignite-pay-core/src/didcomm.rs`

**DIDComm Message Field Changes**:
- `payment-auth-request` added: `available_payment_methods: string[]`
- `payment-auth-response` added: `payment_method: "session_key" | "magicblock" | "relayer"`

---

### F17: User Scans Merchant QR Code Payment (QR → Phone → MCP → Execute Payment)

**Status**: ✅ Implemented

After the user scans the merchant's QR code and selects a payment method (Session Key / MagicBlock), the phone creates a DIDComm `qr-payment-request` message, sends it to MCP via the mediator, MCP executes the payment and returns `qr-payment-response`. After successful payment, MCP also notifies the merchant MCP via DIDComm, triggering the merchant App's voice announcement.

**Complete Flow**:

```
Merchant          Phone              Mediator           MCP              Solana
  |                 |                    |                |                  |
  | [Display QR]    |                    |                |                  |
  |  merchant_did   |                    |                |                  |
  |  amount         |                    |                |                  |
  |  order_id       |                    |                |                  |
  |  mediator_url   |                    |                |                  |
  |                 |                    |                |                  |
  |                 | [Scan QR]          |                |                  |
  |                 | Parse PaymentQrData|                |                  |
  |                 |                    |                |                  |
  |                 | [Display payment details]           |                  |
  |                 | Amount, merchant, order             |                  |
  |                 | [Select payment method]             |                  |
  |                 | ○ Session Key      |                |                  |
  |                 | ○ MagicBlock       |                |                  |
  |                 |                    |                |                  |
  |                 | [User confirms payment]             |                  |
  |                 |                    |                |                  |
  |                 | build_qr_payment_  |                |                  |
  |                 | request            |                |                  |
  |                 | (JWE encrypted)    |                |                  |
  |                 |                    |                |                  |
  |                 |-- qr-payment-req ->|                |                  |
  |                 |   (via WS)         |                |                  |
  |                 |                    |                |                  |
  |                 |                    |-- forward ---->|                  |
  |                 |                    |                |                  |
  |                 |                    |                | [MCP decrypts message]
  |                 |                    |                | QrPaymentCommand |
  |                 |                    |                |                  |
  |                 |                    |                | execute_payment_  |
  |                 |                    |                | auto(method)     |
  |                 |                    |                |                  |
  |                 |                    |                |--- session key ->|
  |                 |                    |                |   or MB voucher  |
  |                 |                    |                |<-- tx sig/voucher|
  |                 |                    |                |                  |
  |                 |                    |                | build_qr_payment_ |
  |                 |                    |                | response         |
  |                 |                    |                |                  |
  |                 |                    |<-- qr-payment- |                  |
  |                 |                    |    response ---|                  |
  |                 |                    |    (JWE)       |                  |
  |                 |                    |                |                  |
  |                 |<-- qr-payment- ---|                |                  |
  |                 |    response        |                |                  |
  |                 |                    |                |                  |
  |                 | [Display payment result]            |                  |
  |                 | Success/Failure    |                |                  |
  |                 |                    |                |                  |
  |                 |                    |                | [After payment success]
  |                 |                    |                | build_qr_payment_ |
  |                 |                    |                | notify           |
  |                 |                    |                |                  |
  |                 |                    |                |-- qr-payment- -->|
  |                 |                    |                |   notify (JWE)   |
  |                 |                    |                |   → Merchant MCP |
  |<-- channel-payment-confirm ---------|<---------------|                  |
  |   (via merchant mediator)           |                |                  |
  |                 |                    |                |                  |
  | [Merchant App voice announcement]   |                |                  |
  | "Payment received X.XX USDC"        |                |                  |
```

**DIDComm Message Types**:
- `ignite-pay/1.0/qr-payment-request` — Phone → MCP (user-initiated payment request after scanning QR, includes `merchant_mediator_url`)
- `ignite-pay/1.0/qr-payment-response` — MCP → Phone (payment result)
- `ignite-pay/1.0/qr-payment-notify` — Buyer MCP → Merchant MCP (notify merchant after successful payment)

**qr-payment-request Message Format**:
```json
{
  "type": "https://didcomm.org/ignite-pay/1.0/qr-payment-request",
  "from": "did:ignite:zPhone...",
  "to": ["did:ignite:zMCP..."],
  "body": {
    "merchant_did": "did:ignite:zMerchant...",
    "amount": 500000000,
    "description": "Coffee",
    "order_id": "uuid-v4",
    "payment_method": "session_key",
    "token": "SOL",
    "merchant_mediator_url": "https://merchant-relay.example.com/"
  }
}
```

**qr-payment-response Message Format**:
```json
{
  "type": "https://didcomm.org/ignite-pay/1.0/qr-payment-response",
  "from": "did:ignite:zMCP...",
  "to": ["did:ignite:zPhone..."],
  "body": {
    "order_id": "uuid-v4",
    "success": true,
    "payment_proof": "Tx: abc123...",
    "payment_method": "session_key"
  }
}
```

**qr-payment-notify Message Format** (Buyer MCP → Merchant MCP):
```json
{
  "type": "https://didcomm.org/ignite-pay/1.0/qr-payment-notify",
  "from": "did:ignite:zBuyerMcp...",
  "to": ["did:ignite:zMerchantMcp..."],
  "body": {
    "order_id": "uuid-v4",
    "amount": 500000000,
    "payment_method": "session_key",
    "payment_proof": "Tx: abc123..."
  }
}
```

**Payment Method Corresponding MCP Execution Path**:

| User Selection | MCP Execution Action |
|---------------|---------------------|
| `session_key` | Sign on-chain SOL/SPL transfer using session key |
| `magicblock` | Off-chain voucher signing: `SHA256(channel_id ‖ seq ‖ amount)` + Ed25519 |
| `relayer` | Session key signs transaction, relayer submits and pays gas |

**Code Locations**:
- DIDComm message construction: `ignite-pay-core/src/didcomm.rs` — `build_qr_payment_request()`, `build_qr_payment_response()`, `build_qr_payment_notify()`
- MCP mediator handling: `ignite-pay-mcp/src/mediator.rs` — `QrPaymentCommand` + `qr-payment-request` handler + `send_to_mediator()`
- MCP payment execution: `ignite-pay-mcp/src/main.rs` — QR payment handler background task (includes merchant notification)
- Phone-side sending: `ignite_pay_app/rust/src/api/simple.rs` — `send_qr_payment_request()`
- Phone-side QR parsing: `ignite_pay_app/rust/src/api/channel.rs` — `parse_payment_qr()` (includes `merchant_mediator_url`)
- `send_to_phone()`: `ignite-pay-mcp/src/mediator.rs` — generic DIDComm message sending method
- Merchant MCP handling: `ignite-pay-merchant-mcp/src/mediator.rs` — `qr-payment-notify` handler → `channel-payment-confirm` → Merchant App

**Difference from F16 (Payment Method Selection)**:
- F16: MCP asks phone user to choose method during x402 payment authorization
- F17: Phone directly chooses method when scanning QR, sends to MCP for execution
- Both share the `PaymentMethod` enum and `execute_payment_auto()` dispatch logic

---

### F18: Merchant Voice Announcement (Notify Merchant MCP After QR Payment Success → Merchant App Announcement)

**Status**: ✅ Implemented

After a successful QR code payment, the buyer MCP notifies the merchant MCP via DIDComm `qr-payment-notify` message. The merchant MCP then notifies the merchant App via `channel-payment-confirm`, triggering a voice announcement ("Payment received X.XX USDC").

**Complete Flow**:

```
Buyer MCP                  Merchant Mediator         Merchant MCP         Merchant App
    |                            |                        |                     |
    | QR payment execution success|                        |                     |
    |                            |                        |                     |
    | build_qr_payment_notify    |                        |                     |
    | (order_id, amount, method, |                        |                     |
    |  payment_proof)            |                        |                     |
    |                            |                        |                     |
    | pack_encrypted(merchant_did)|                       |                     |
    |                            |                        |                     |
    |--- forward(JWE) ---------->|                        |                     |
    |   POST merchant_mediator   |                        |                     |
    |   _url                     |                        |                     |
    |                            |                        |                     |
    |                            |-- deliver to --------->|                     |
    |                            |   merchant DID         |                     |
    |                            |                        |                     |
    |                            |                        | Decrypt qr-payment- |
    |                            |                        | notify              |
    |                            |                        |                     |
    |                            |                        | build_channel_      |
    |                            |                        | payment_confirm     |
    |                            |                        |                     |
    |                            |                        |--- channel-payment ->|
    |                            |                        |   confirm (JWE)     |
    |                            |                        |                     |
    |                            |                        |                     | Voice announcement:
    |                            |                        |                     | "Payment received
    |                            |                        |                     |  X.XX USDC"
```

**Prerequisites**:
1. QR code contains `merchant_mediator_url` field (HTTP URL of merchant mediator)
2. Merchant MCP is paired with merchant App
3. Merchant App has `VoiceService` enabled (flutter_tts)

**DIDComm Message Types**:
- `ignite-pay/1.0/qr-payment-notify` — Buyer MCP → Merchant MCP (payment success notification)
- `ignite-pay/1.0/channel-payment-confirm` — Merchant MCP → Merchant App (trigger voice announcement)

**qr-payment-notify Message Format**:
```json
{
  "type": "https://didcomm.org/ignite-pay/1.0/qr-payment-notify",
  "from": "did:ignite:zBuyerMcp...",
  "to": ["did:ignite:zMerchantMcp..."],
  "body": {
    "order_id": "uuid-v4",
    "amount": 500000000,
    "payment_method": "session_key",
    "payment_proof": "Tx: abc123..."
  }
}
```

**Code Locations**:
- DIDComm message construction: `ignite-pay-core/src/didcomm.rs` — `build_qr_payment_notify()`
- Buyer MCP sending: `ignite-pay-mcp/src/main.rs` — QR payment handler (sends notify after successful payment)
- Buyer MCP mediator: `ignite-pay-mcp/src/mediator.rs` — `send_to_mediator()` method
- `QrPaymentCommand`: `ignite-pay-mcp/src/mediator.rs` — includes `merchant_mediator_url` field
- Merchant MCP handling: `ignite-pay-merchant-mcp/src/mediator.rs` — `qr-payment-notify` handler → `channel-payment-confirm`
- QR data parsing: `ignite_pay_app/rust/src/api/channel.rs` — `PaymentQrData.merchant_mediator_url`
- Merchant App voice: `ignite_pay_merchant_app/lib/services/voice_service.dart` — `VoiceService` (flutter_tts)

**QR Code JSON Format (New Field)**:
```json
{
  "type": "ignite-pay-request",
  "merchant_did": "did:ignite:z...",
  "amount": 500000000,
  "description": "Coffee",
  "order_id": "uuid-v4",
  "hub_endpoint": "https://...",
  "timestamp": 1700000000,
  "merchant_mb_pubkey": "",
  "merchant_mediator_url": "https://merchant-relay.example.com/"
}
```

**Difference from MagicBlock Voucher Flow**:
- MB voucher flow: Phone → Merchant MCP (`mb-voucher`) → verify signature → confirm order → `channel-payment-confirm` → voice announcement
- QR payment notification flow: Buyer MCP → Merchant MCP (`qr-payment-notify`) → direct confirmation → `channel-payment-confirm` → voice announcement
- Difference: MB voucher requires merchant to verify buyer's signature; QR notification is based directly on the buyer MCP trust relationship (already encrypted and authenticated)

---

## 3. Complete DIDComm Message Type List

### Defined Message Types

| Message Type | Direction | Purpose |
|-------------|-----------|---------|
| `ignite-pay/1.0/connection-request` | Phone → MCP | Pairing request |
| `ignite-pay/1.0/connection-response` | MCP → Phone | Pairing response |
| `ignite-pay/1.0/connection-confirm` | Phone → MCP | Pairing confirmation |
| `ignite-pay/1.0/connection-confirm-response` | MCP → Phone | Final pairing confirmation |
| `ignite-pay/1.0/payment-auth-request` | MCP → Phone | Payment authorization request (includes `available_payment_methods`) |
| `ignite-pay/1.0/payment-auth-response` | Phone → MCP | Payment authorization response (includes session key + `payment_method`) |
<!-- State Channel: Exploration phase, not enabled
| `ignite-pay/1.0/channel-payment-request` | MCP → Phone | State channel payment request |
| `ignite-pay/1.0/channel-payment-confirm` | MCP → Phone | State channel payment confirmation |
| `ignite-pay/1.0/create-channel-request` | Phone → MCP | Request to create state channel |
| `ignite-pay/1.0/create-channel-response` | MCP → Phone | State channel creation response |
-->
| `ignite-pay/1.0/list-sync-notification` | MCP → Phone | Whitelist/blacklist change notification |
| `ignite-pay/1.0/mb-voucher` | Phone → Merchant | MagicBlock voucher sent to merchant |
| `ignite-pay/1.0/qr-payment-request` | Phone → MCP | Payment request initiated after user scans QR (includes payment_method + merchant_mediator_url) |
| `ignite-pay/1.0/qr-payment-response` | MCP → Phone | QR payment result (includes payment_proof) |
| `ignite-pay/1.0/qr-payment-notify` | Buyer MCP → Merchant MCP | Notify merchant MCP after QR payment success (triggers merchant App voice announcement) |

### Message Types to Be Added

| Message Type | Direction | Purpose | Related Flow |
|-------------|-----------|---------|-------------|
| `payment-auth-request` extension | MCP → Phone | Add `available_payment_methods`, `new_session_key` fields | F2, F16 ✅ Implemented |
| `session-fund-request` | MCP → Phone | Request top-up when balance is insufficient (reuse `payment-auth-request` or standalone message) | F3 ✅ Implemented |
| `session-fund-response` | Phone → MCP | Top-up result (funded + tx sig / rejected) | F3 ✅ Implemented |
| `ignite-pay/1.0/merchant-auth-request` | MCP → Phone | New merchant authorization / limit increase request | F8 ✅ Implemented |
| `ignite-pay/1.0/merchant-auth-response` | Phone → MCP | Merchant authorization result | F8 ✅ Implemented |
| `ignite-pay/1.0/balance-notification` | MCP → Phone | Insufficient balance warning | F13 ✅ Implemented |
| `ignite-pay/1.0/session-renew-request` | MCP → Phone | Session key about to expire, request renewal (MCP creates new key → sends to phone for top-up) | F14 ✅ Implemented |
| `ignite-pay/1.0/session-renew-response` | Phone → MCP | New session key top-up completion confirmation | F14 ✅ Implemented |

---

## 4. Priority Recommendations

| Priority | Flow | Reason |
|----------|------|--------|
| **P0** | F2: Session Key Creation (embedded in payment flow) | No session key at payment time → MCP creates → sends to phone along with payment request → phone handles registration + top-up + authorization in one step |
| **P0** | F3: Insufficient Balance → Top-up Request | Common cause of payment failure, needs a closed loop |
| **P1** | F8: Merchant Unauthorized → Additional Authorization | Core to security and user experience |
| **P2** | F14: Session Key Renewal | MCP creates new key → phone tops up, prevents payment interruption |
| **P2** | F13: Balance Query & Notification | Proactive maintenance, reduces payment failures |
| **P3** | F15: Multi-Merchant Concurrent Payments | Performance optimization, not a functional blocker |

> **Principle**: Session keys can only be created locally by MCP (MCP exclusively holds the private key). The phone is only responsible for top-up (SOL gas + stablecoins). The phone should not create session keys.
