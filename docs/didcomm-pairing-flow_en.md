# DIDComm Pairing Flow

This document describes the pairing flow for establishing the initial DID connection between ignite-pay-mcp / ignite-pay-merchant-mcp and the mobile app. Both the user side and the merchant side use the same pairing protocol.

## 1. Unified Pairing Flow

### 1.1 Sequence Diagram

```
MCP Server                      MCP's Mediator              Phone App
(ignite-pay-mcp                                              (ignite_pay_app
 or merchant-mcp)              Phone's Mediator             or merchant app)
   │                                  │                              │
   │─── WS connect ──────────────────>│                              │
   │<── ws-challenge (nonce) ─────────│                              │
   │─── ws-challenge-response ───────>│  Ed25519 signature + DID Document
   │<── ws-auth-ok ───────────────────│                              │
   │─── mediate-request ─────────────>│                              │
   │<── mediate-grant ────────────────│                              │
   │─── keylist-update ──────────────>│  Register routing keys       │
   │<── keylist-update-response ──────│                              │
   │─── peer-introduction ───────────>│  Share DID Document          │
   │                                  │                              │
   │ [Generate QR: didcomm://?_oob=<b64>]                            │
   │  QR contains HTTP URL (not WSS)  │                              │
   │                                  │                   [Scan QR]  │
   │                                  │                              │
   │                                  │<──── WS connect ────────────│ (phone's mediator)
   │                                  │───── ws-challenge ──────────>│
   │                                  │<──── ws-challenge-response ─│
   │                                  │───── ws-auth-ok ────────────>│
   │                                  │<──── mediate-request ───────│
   │                                  │───── mediate-grant ─────────>│
   │                                  │<──── keylist-update ────────│
   │                                  │───── keylist-update-res ────>│
   │                                  │<──── peer-introduction ─────│
   │                                  │                              │
   │                                  │                   [phone sends connection-request]
   │                                  │                   Contains did_document, mediator_http_url
   │                                  │                              │
   │                   [phone HTTP POST to MCP's mediator]           │
   │<── forward(connection-request) ──│<──── HTTP POST ─────────────│
   │                                  │                              │
   │ [Store phone_did, phone_mediator_http_url, register peer]       │
   │ [Mark as pending_phone]          │                              │
   │                                  │                              │
   │═══ Three-step Handshake Step 2: MCP → App ═══                   │
   │                                  │                              │
   │─── forward(connection-response) ─│───── HTTP POST ────────────>│
   │    {accepted, did_document,      │    To App's mediator         │
   │     mediator_http_url,           │                              │
   │     mcp_nonce, mcp_signature}    │                              │
   │                                  │                              │
   │                                  │ [Verify MCP signature]       │
   │                                  │ [Store PairedMcp]            │
   │                                  │ [App side pairing complete ✓]│
   │                                  │                              │
   │═══ Three-step Handshake Step 3: App → MCP ═══                   │
   │                                  │                              │
   │                                  │<──── HTTP POST ─────────────│
   │<── forward(connection-confirm) ──│    To MCP's mediator         │
   │    {phone_nonce, phone_signature}│                              │
   │                                  │                              │
   │ [Verify App signature]           │                              │
   │ [pending → paired]               │                              │
   │ [MCP side pairing complete ✓]    │                              │
   │                                  │                              │
   │═══ Pairing Complete ═══          │                              │
```

### 1.2 Message Routing Architecture

**Direct connection to the peer's mediator**: Each participant directly connects to the other party's mediator to send messages. There is no mediator-to-mediator forwarding.

- **App → MCP**: App sends forward-wrapped messages via HTTP POST to MCP's mediator
- **MCP → App**: MCP sends forward-wrapped messages via HTTP POST to App's mediator
- **Same mediator optimization**: If both parties use the same mediator, messages are sent directly through their respective WS connections (no temporary connection needed)

Forward message format (DIDComm Routing 2.0 protocol):

```json
{
  "type": "https://didcomm.org/routing/2.0/forward",
  "id": "fwd-<uuid>",
  "body": { "next": "<target_did>" },
  "attachments": [{
    "data": { "json": "<encrypted_jwe_or_plaintext>" }
  }]
}
```

### 1.3 Step-by-Step Details

#### Step 1: MCP Starts and Connects to Mediator

When MCP starts, `MediatorConnection::connect_and_run()` generates a `did:ignite:z<Base58(Ed25519_pubkey)>` identity and connects to the mediator via WebSocket.

**Authentication Handshake (Phase 0)**:

| Direction | Message Type | Description |
|-----------|-------------|-------------|
| Mediator → MCP | `ws-challenge` | Sends nonce |
| MCP → Mediator | `ws-challenge-response` | Ed25519-signed nonce + DID Document |
| Mediator → MCP | `ws-auth-ok` | Authentication successful |

**Mediation Handshake (Phase A)**:

| Direction | Message Type | Protocol |
|-----------|-------------|----------|
| MCP → Mediator | `mediate-request` | `coordinate-mediation/2.0` |
| Mediator → MCP | `mediate-grant` | |
| MCP → Mediator | `keylist-update` | Register DID routing keys |
| MCP → Mediator | `peer-introduction` | `peer-did-discovery/1.0`, share DID Document |

#### Step 2: MCP Generates OOB Invitation (QR Code)

- If no paired phone exists at startup, a QR code is automatically generated and printed
- The QR code contains an **HTTP URL** (`https://...`), not a WSS URL
- The App can use it directly for HTTP POST message sending without conversion

QR content: `didcomm://?_oob=<base64url(JSON)>`

OOB Invitation message structure (simplified, without DID Document):

```json
{
  "type": "https://didcomm.org/out-of-band/2.0/invitation",
  "from": "did:ignite:z...",
  "body": {
    "services": [{
      "service_endpoint": "https://mediator.example.com",
      "routing_keys": ["did:ignite:z..."]
    }]
  }
}
```

Key point: `service_endpoint` is an HTTP URL that the App uses directly for POST messages.

#### Step 3: App Scans QR Code and Connects to Mediator

The App detects the `didcomm://` prefix and parses the OOB invitation:
1. Base64url-decode the `_oob` parameter
2. Extract the MCP DID and mediator HTTP address
3. Connect to its own mediator (perform ws-challenge + coordinate-mediation handshake)

#### Step 4: App Sends connection-request (Three-step Handshake Step 1)

```json
{
  "type": "https://didcomm.org/ignite-pay/1.0/connection-request",
  "from": "did:ignite:z<app>",
  "to": ["did:ignite:z<mcp>"],
  "body": {
    "push_channel": "websocket",
    "fcm_token": "...",
    "mediator_http_url": "https://phone-mediator.example.com",
    "did_document": { ... }
  }
}
```

Key fields:
- `mediator_http_url`: The App informs MCP of its own mediator HTTP address
- `did_document`: The App's DID Document, enabling MCP to communicate securely

Sending method: The App wraps the connection-request in a forward message and HTTP POSTs it to MCP's mediator.

#### Step 5: MCP Sends connection-response (Three-step Handshake Step 2)

After receiving the connection-request, MCP:
1. Extracts the App DID and registers it as an encryption peer
2. Extracts `mediator_http_url` and saves it
3. **Stores as pending status**
4. Generates a random nonce and signs it with Ed25519
5. Sends connection-response (with signature) back to the App

```json
{
  "type": "https://didcomm.org/ignite-pay/1.0/connection-response",
  "from": "did:ignite:z<mcp>",
  "to": ["did:ignite:z<app>"],
  "body": {
    "accepted": true,
    "did_document": { ... },
    "mediator_http_url": "https://mcp-mediator.example.com",
    "mcp_nonce": "<random_uuid>",
    "mcp_signature": "<base64_no_pad_ed25519_signature>"
  }
}
```

#### Step 6: App Verifies MCP Signature and Sends connection-confirm (Three-step Handshake Step 3)

After receiving the connection-response, the App:
1. Verifies the MCP signature using `verify_did_signature(mcp_did, mcp_nonce, mcp_signature)`
2. If verification succeeds: stores PairedMcp (DID, DID Doc, mediator HTTP URL), **App side pairing is complete**
3. Generates its own random nonce and signs it with Ed25519
4. Sends connection-confirm to MCP

```json
{
  "type": "https://didcomm.org/ignite-pay/1.0/connection-confirm",
  "from": "did:ignite:z<app>",
  "to": ["did:ignite:z<mcp>"],
  "body": {
    "phone_nonce": "<random_nonce>",
    "phone_signature": "<base64_no_pad_ed25519_signature>"
  }
}
```

#### Step 7: MCP Verifies App Signature, Pairing Complete

After receiving the connection-confirm, MCP:
1. Checks for a pending pairing for that DID
2. Verifies the App's signature using `verify_did_signature(phone_did, nonce, signature)`
3. If verification succeeds: pending → paired, **MCP side pairing is complete**
4. If verification fails: clears the pending entry

**Security Notes**:
- DIDComm messages are authenticated via Ed25519 signatures and cannot be forged
- Bidirectional signature verification: MCP signs in Step 2 to prove identity, App signs in Step 3 to prove identity
- The App completes pairing first (after verifying MCP's signature), MCP completes second (after verifying App's signature)
- Pairing messages are transmitted via mediator as plaintext JSON (not JWE-encrypted); security is ensured by signatures

---

## 2. Differences Between the Two Sides

### 2.1 Service Ports

| Service | Default Port | Description |
|---------|-------------|-------------|
| didcomm-router (user) | 8080 | User-side Mediator |
| didcomm-router (merchant) | 4000 | Merchant-side Mediator |
| ignite-pay-hub-registry | 3004 | Hub registration service |

### 2.2 Server-side (MCP) Implementation Status

| Component | ignite-pay-mcp | ignite-pay-merchant-mcp |
|-----------|---------------|------------------------|
| WS challenge-response authentication | Yes | Yes (simplified) |
| Coordinate-mediation handshake | Yes | Yes |
| OOB invitation generation | Yes | Yes |
| QR code generation | Yes | Yes |
| connection-request handling | Yes | Yes |
| Three-step handshake bidirectional signature verification | Yes | Yes |
| Auto-print QR at startup | Yes | Yes |

### 2.3 Client-side (App) Implementation Status

| Component | ignite_pay_app | ignite_pay_merchant_app |
|-----------|---------------|------------------------|
| QR scanning (mobile_scanner) | Implemented | Not yet implemented |
| `didcomm://` OOB parsing | Implemented | Not yet implemented |
| WS challenge-response authentication | Implemented | Not yet implemented |
| Coordinate-mediation handshake | Implemented | Not yet implemented |
| connection-request sending | Implemented | Not yet implemented |
| MCP signature verification | Implemented | Not yet implemented |
| payment-auth-request receiving | Implemented | Not yet implemented |

---

## 3. DIDComm Message Types Summary

### 3.1 Pairing-related

| Message Type | Direction | Description |
|-------------|-----------|-------------|
| `out-of-band/2.0/invitation` | MCP → App (via QR) | OOB invitation, contains DID and mediator HTTP address |
| `ignite-pay/1.0/connection-request` | App → MCP | Pairing request, contains did_document and mediator_http_url |
| `ignite-pay/1.0/connection-response` | MCP → App | Pairing response, contains did_document, mediator_http_url, mcp_nonce, and mcp_signature |
| `ignite-pay/1.0/connection-confirm` | App → MCP | Signature confirmation, contains phone_nonce + phone_signature |

### 3.2 Message Routing

| Message Type | Protocol | Description |
|-------------|----------|-------------|
| `routing/2.0/forward` | Standard DIDComm | Wrapped message; mediator routes to target DID based on `body.next` |

### 3.3 Mediator Handshake

| Message Type | Protocol | Description |
|-------------|----------|-------------|
| `ignite-pay/1.0/ws-challenge` | Custom | WS authentication challenge (contains nonce) |
| `ignite-pay/1.0/ws-challenge-response` | Custom | Ed25519 signature + DID Document |
| `ignite-pay/1.0/ws-auth-ok` | Custom | Authentication success confirmation |
| `coordinate-mediation/2.0/mediate-request` | Standard DIDComm | Request mediation |
| `coordinate-mediation/2.0/mediate-grant` | Standard DIDComm | Grant mediation |
| `coordinate-mediation/2.0/keylist-update` | Standard DIDComm | Register routing keys |
| `peer-did-discovery/1.0/discover` | Custom | Share DID Document |

### 3.4 Message Pickup (Offline Messages)

| Message Type | Protocol | Description |
|-------------|----------|-------------|
| `messagepickup/3.0/status-request` | Standard DIDComm | Query queued message count |
| `messagepickup/3.0/batch-pickup` | Standard DIDComm | Batch fetch offline messages |

### 3.5 Payment Flow (Used After Pairing)

| Message Type | Direction | Description |
|-------------|-----------|-------------|
| `ignite-pay/1.0/payment-auth-request` | MCP → App | Payment authorization request |
| `ignite-pay/1.0/payment-auth-response` | App → MCP | Payment authorization response (contains session key) |
| `ignite-pay/1.0/create-channel-request` | Merchant App → MCP | State channel creation request |
| `ignite-pay/1.0/channel-payment-confirm` | MCP → Merchant App | Channel payment confirmation |
| `ignite-pay/1.0/session-fund-request` | MCP → Phone | F3/F7: Request funding when session balance insufficient |
| `ignite-pay/1.0/session-fund-response` | Phone → MCP | F3/F7: Funding completed response |
| `ignite-pay/1.0/balance-notification` | MCP → Phone | F13: Balance below threshold notification |
| `ignite-pay/1.0/session-renew-request` | MCP → Phone | F14: Request session key renewal |
| `ignite-pay/1.0/session-renew-response` | Phone → MCP | F14: Renewal completed response |

---

## 4. Design Characteristics

- **Unified pairing protocol**: Both user side and merchant side use the same OOB invitation + connection-request pairing flow
- **Non-standard DIDExchange**: Uses custom `ignite-pay/1.0/connection-request` instead of the Aries DIDExchange protocol
- **HTTP URL embedded directly in QR**: The OOB invitation contains an HTTP URL directly; the App needs no conversion
- **Direct connection to peer's mediator**: Each participant directly connects to the other party's mediator to send forward-wrapped messages
- **Bidirectional mediator address exchange**: The App informs MCP of its mediator HTTP address in the connection-request
- **DID Document embedded in request**: App and MCP provide each other with DID Documents in their requests
- **Three-step handshake bidirectional signature verification**:
  1. MCP sends a signed nonce in the connection-response
  2. App verifies MCP's signature, stores pairing info, and sends its own signed nonce
  3. MCP verifies App's signature and completes pairing
- **Pending state**: MCP marks the connection-request as pending upon receipt; pairing completes only after verifying the App's signature
- **Plaintext JSON message handling**: Pairing messages are transmitted via mediator as plaintext; security is ensured by Ed25519 signatures
- **Persistent pairing**: The App side uses SharedPreferences to store PairedMcp; the MCP side uses sled for storage

---

## 5. Key Files

### 5.1 Shared Protocol Library

| File | Responsibility |
|------|---------------|
| `ignite-pay-core/src/didcomm.rs` | DIDComm message construction, OOB invitation, pack/unpack |
| `ignite-pay-core/src/identity.rs` | `did:ignite` generation, DID Document construction, sign/verify |

### 5.2 User Side

| File | Responsibility |
|------|---------------|
| `ignite-pay-mcp/src/mediator.rs` | MCP mediator connection, OOB invitation generation, inbound message handling, three-step handshake |
| `ignite-pay-mcp/src/main.rs` | `generate_pairing_invitation` tool, auto QR at startup |
| `ignite_pay_app/lib/qr_scanner_screen.dart` | QR scanning UI |
| `ignite_pay_app/lib/services/didcomm_service.dart` | `parseInvitationAndConnect()`, mediator connection, signature verification |
| `ignite_pay_app/rust/src/api/ws_client.rs` | Rust WS client: mediator handshake, message queue |
| `ignite_pay_app/rust/src/api/simple.rs` | `sign_nonce()`, `verify_did_signature()`, HTTP authentication |

### 5.3 Merchant Side

| File | Responsibility |
|------|---------------|
| `ignite-pay-merchant-mcp/src/mediator.rs` | Merchant MCP mediator connection, OOB invitation generation, three-step handshake |
| `ignite-pay-merchant-mcp/src/main.rs` | Auto-generate QR at startup |
| `ignite_pay_merchant_app/rust/src/api/merchant_didcomm.rs` | Merchant App DIDComm communication |
