**Minimal Mutual-Trust Connection Scheme Based on DIDComm-V2 OOB (Out-of-Band) Protocol**

This scheme completely removes the mandatory dependency on the Solana on-chain Root DID, and instead achieves secure handshake and command dispatch between the mobile client and the MCP Server through a **Peer-to-Peer (P2P)** model.

---

# Client-Side: Minimal Connection Scheme Based on DIDComm-V2 OOB

## 1. Core Design Principles
* **Zero On-Chain Dependency**: No need to register a Root DID on-chain, saving gas fees and lowering the barrier for users.
* **Trust On First Use (TOFU)**: Initial trust is established through QR code scanning (out-of-band communication).
* **Bidirectional Peer Identity**: Both the mobile app and the Server hold independent `did:ignite` identifiers.
* **Relay Forwarding**: A Mediator service enables public-network asynchronous communication (WebSocket + HTTP).

---

## 2. Roles and Identity Definitions

| Entity | Identity Type | Responsibilities |
| :--- | :--- | :--- |
| **Mobile App** | `did:ignite:z<multibase>` | Initiates control requests, manages the list of connected Servers. |
| **MCP Server** | `did:ignite:z<multibase>` | Listens for Mediator messages, executes MCP Tools and returns results. |
| **Mediator (didcomm-router)** | Mediator / Relay | Message forwarding routing, offline queueing, FCM push notifications. Does not participate in decryption. |

### DID Identifier Format

The system uses a custom `did:ignite` method (not `did:peer`), in the format:

```
did:ignite:z<multibase-encoded-public-key>
```

Encoding rules:
1. Ed25519 public key (32 bytes) → add multicodec prefix `[0xed, 0x01]` → 34 bytes total
2. Base58 encode → prepend prefix `did:ignite:z`

> Example: `did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK`

### DID Document Structure

Each `did:ignite` identifier corresponds to a W3C DID Document containing an Ed25519 signing key and an X25519 key agreement key:

```json
{
  "@context": ["https://www.w3.org/ns/did/v1"],
  "id": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
  "verificationMethod": [{
    "id": "did:ignite:z6Mkha...#key-signing-1",
    "type": "Ed25519VerificationKey2020",
    "controller": "did:ignite:z6Mkha...",
    "publicKeyMultibase": "z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK"
  }],
  "keyAgreement": [{
    "id": "did:ignite:z6Mkha...#key-agreement-1",
    "type": "X25519KeyAgreementKey2020",
    "controller": "did:ignite:z6Mkha...",
    "publicKeyBase64": "<base64-encoded-X25519-public-key>"
  }]
}
```

- **Ed25519**: Used for identity signing (`verificationMethod`), proving "who I am"
- **X25519**: Used for message encryption (`keyAgreement`), responsible for ECDH key agreement

---

## 3. Connection Establishment Flow (OOB Handshake)

### 3.1 Server Generates Invitation (Out-of-Band Invitation)
After the MCP Server starts, if no device is bound, it generates an **OOB invitation package** via CLI or log output.

**Generation process** (`ignite-pay-mcp/src/mediator.rs`):
1. Call `build_oob_invitation(our_did, label, mediator_ws_url, did_doc)` to construct the OOB message
2. JSON-serialize the message, base64url-encode it
3. Format as `didcomm://?_oob=<base64url>` URL
4. Optional: call `generate_invitation_qr()` to generate an ASCII QR code

**OOB Invitation Message Format**:
```json
{
  "type": "https://didcomm.org/out-of-band/2.0/invitation",
  "id": "<unique-id>",
  "from": "did:ignite:z<server-did>",
  "body": {
    "label": "Ignite Pay MCP",
    "goal_code": "p2p-messaging",
    "accept": ["didcomm/v2"],
    "did_document": { ... },
    "services": [{
      "id": "#mediator",
      "type": "did-communication",
      "service_endpoint": "wss://relay.ignite-pay.com/ws",
      "routing_keys": ["did:ignite:z<server-did>"]
    }]
  }
}
```

**QR Code URL Format**: `didcomm://?_oob=<base64url-encoded-invitation-json>`

### 3.2 Mobile Client Scans QR Code and Responds
1. **Scan QR code**: The mobile App parses the OOB invitation URL, base64url-decodes it, and extracts the Server's `did:ignite`, DID Document, and Mediator WebSocket address.
2. **Generate local DID**: The App calls `generate_ignite_did()` to create an independent `did:ignite` identifier for this Server (Ed25519 + X25519 key pair).
3. **Connect to Mediator**: Establish a WebSocket connection to the Mediator and complete the WS authentication handshake (challenge-response).
4. **Send connection-request**: Send a `https://didcomm.org/ignite-pay/1.0/connection-request` message to the Server via the Mediator (includes push channel and optional FCM token).

### 3.3 Establishing Mutual Trust (White-listing)
* **Server-side verification**: Upon receiving a `connection-request`, the Server registers the mobile client's `did:ignite` via `add_peer_from_doc()` into its local DIDComm Agent, recording it in a whitelist.
* **Server-side response**: Returns a `connection-response` (`accepted: true/false`).
* **Subsequent communication**: Both parties communicate using Authcrypt (JWE authenticated encryption). During each encryption, the sender uses its own X25519 private key and the recipient's X25519 public key to perform ECDH key agreement, deriving a symmetric encryption key (AES-256-GCM or XChaCha20-Poly1305), without requiring a separate negotiation step.

---

## 4. Command Routing and Security Protocol (DIDComm V2)

### 4.1 Message Encapsulation Model
The message encapsulation layers for secure transmission through the Mediator:

1. **Inner Layer (MCP Payload)**: Raw business data (e.g., payment authorization request, authorization response).
2. **Encryption Layer (Authcrypt JWE)**: The sender uses the `pack_authcrypt()` method from the `affinidi-messaging-didcomm` library, performing ECDH + AES-GCM encryption based on both parties' X25519 keys, producing a JWE.
3. **Forward Layer (Routing)**: The encrypted JWE is placed as the body of a `https://didcomm.org/routing/2.0/forward` message, sent to the Mediator, which routes it to the target recipient.

### 4.2 MCP Command Interaction Examples
* **Mobile -> Server**:
  `App -> pack_authcrypt(MCP_Call) -> Forward -> Mediator -> Server -> unpack()`
* **Server -> Mobile**:
  `Server -> pack_authcrypt(MCP_Result) -> Forward -> Mediator -> App -> unpack()`

### 4.3 Mediator Handshake Protocol
When the MCP Server connects to the Mediator, it performs the following handshake flow (all in plaintext):

| Step | Message Type | Description |
|:---|:---|:---|
| 1 | `mediate-request` | Request the Mediator to provide relay service for this DID |
| 2 | `mediate-grant` | Mediator grants authorization |
| 3 | `keylist-update` | Register DID routing information |
| 4 | `keylist-update-response` | Confirm registration |
| 5 | `peer-introduction` | Send DID Document for the Mediator to forward |
| 6 | `status-request` | Query the number of offline messages |
| 7 | `batch-pickup` | Fetch offline messages |

After the handshake completes, a bidirectional message loop begins.

---

## 5. Implementation Notes

### 5.1 Mobile Client (App)
* **Persistence**: Use secure storage (KeyChain/EncryptedSharedPreferences) to save the `did:ignite` private keys for each Server (Ed25519 signing key + X25519 key agreement key).
* **Offline Message Retrieval**: Pull offline messages via the Mediator's REST API:
  - `GET /v1/sync/list` — Paginated query for queued messages
  - `GET /v1/sync/messages/{msg_id}` — Retrieve a single message
* **Message Pickup 3.0**: After the App connects to the Mediator, it uses `status-request` and `batch-pickup` to pull messages accumulated during the offline period.

### 5.2 MCP Server (Rust)
* **Automated Invitation**: Upon initialization, the Server checks for paired devices; if none exist, it automatically generates an OOB QR code.
* **Permission Interceptor**:
  ```rust
  // Actual code: check whitelist before MCP processing
  if !agent.has_peer(sender_did) {
      return Err("Unauthorized_DID");
  }
  ```
* **Risk Control Engine**: Implemented through `ListStore` (`ignite-pay-core/src/list_store.rs`) for whitelist/blacklist management:
  - `risk_check(merchant_did, amount)` -> `Blocked` / `AutoApproved` / `NeedsAuth`
  - Supports entry expiration and amount limits
  - Supports syncing lists across devices via IPFS

### 5.3 Revocation and Permission Management
Permission management is implemented programmatically through `ListStore`, not limited to physical device operations:
* **Whitelist Removal**: `remove_from_whitelist(did)` revokes automatic authorization for a given DID
* **Blacklist Addition**: `add_to_blacklist(did, expires)` blocks a specific DID
* **Risk Downgrade**: After removal from the whitelist, subsequent requests from that DID are downgraded to `NeedsAuth` (requires manual confirmation)
* The Mediator also supports admin-initiated `reset-peers` to reset all pairing relationships

---

## 6. Mediator (didcomm-router) Service

### 6.1 Transport Methods
* **WebSocket** (`GET /ws`): Bidirectional real-time communication, supports online message push
* **HTTP** (`POST /`): Unidirectional message delivery

### 6.2 Authentication Mechanism
* **WebSocket Authentication**: Challenge-response mode. The Mediator sends a plaintext challenge containing a nonce; the client returns a JWE-encrypted challenge response to prove key ownership.
* **REST API Authentication**: JWT Bearer Token. Obtain a nonce via `GET /v1/auth/challenge`, then exchange a DID-signed response for a JWT via `POST /v1/auth/token`.

### 6.3 Push Notifications
* Supports **FCM (Firebase Cloud Messaging)** push. When the recipient is offline, the Mediator enqueues the message and notifies the mobile client via FCM.
* Clients register FCM tokens via `POST /v1/devices/register-token`.

### 6.4 Offline Messages
* The Mediator maintains a message queue for each registered DID (sled persistent storage)
* Supports both real-time push for online clients and pull-based retrieval for offline clients
* Message deduplication: DashMap-based deduplication by message ID, preventing replay attacks

### 6.5 Complete Endpoint List

| Method | Route | Function |
|:---|:---|:---|
| POST | `/` | Receive DIDComm messages (HTTP) |
| GET | `/ws` | WebSocket connection |
| GET | `/health` | Health check |
| GET | `/v1/auth/challenge` | Get authentication nonce |
| POST | `/v1/auth/token` | Exchange DID signature for JWT |
| GET | `/v1/sync/list` | Paginated query for queued messages |
| GET | `/v1/sync/messages/{msg_id}` | Retrieve a single message |
| POST | `/v1/devices/register-token` | Register FCM push token |
| POST | `/v1/agents/bind` | Bind agent to user |
| POST | `/v1/agents/{agent_id}/command` | Command forwarding |

---

## 7. Summary
This scheme leverages the Authcrypt encryption capability of **DIDComm V2**, based on the custom `did:ignite` identifier method, to provide end-to-end encrypted communication security for the MCP Server **without requiring blockchain participation**. It optimizes the user experience, enabling merchants or developers to quickly deploy their own AI Agent control system with a "scan-to-connect" approach.

---

**Developer Notes:**
The system uses `affinidi-messaging-didcomm` (v0.13) as the DIDComm engine. DID identifiers are generated via `generate_ignite_did()` in `ignite-pay-core/src/identity.rs`, and DID Documents are constructed via `build_did_document()`. All DIDComm message encryption/decryption is handled by `pack_encrypted()` and `unpack_message()` in `ignite-pay-core/src/didcomm.rs`.

### Key Dependencies
* **Rust**: `affinidi-messaging-didcomm` — DIDComm V2 message encryption/decryption
* **Key Types**: Ed25519 (signing) + X25519 (key agreement), paired together
* **Encryption Mode**: Authcrypt only (JWE authenticated encryption), Anoncrypt is not implemented

### Developer Checklist
1. **Key Management**: Each connection requires an independent `did:ignite` key pair (Ed25519 + X25519), with private keys stored in secure storage
2. **DID Resolution**: `did:ignite` is a locally-resolved format that does not require an external resolver. Key data is encoded in the DID Document and extracted via `parse_did_document()`
3. **Encryption/Decryption**: The sender encrypts with `pack_authcrypt(sender, recipient)`, and the recipient decrypts with `unpack(jwe)`. ECDH key agreement is completed automatically during encryption, with no separate negotiation step required
