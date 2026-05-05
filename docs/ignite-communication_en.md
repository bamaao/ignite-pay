**Full-Link AI Agent Communication Solution Based on DIDComm V2**.

It integrates **FCM signal wake-up**, **HTTPS active pull**, and **WebSocket real-time communication**, supporting two push channels:

* **International users**: FCM signal wake-up + HTTPS active pull (power-saving, no persistent connection needed).
* **China users**: WebSocket online direct push + offline staging / Pickup pull after reconnection (FCM is unavailable in mainland China).

This is a production-grade architecture designed for high-security, cross-border environments (Google Play & iOS).

---

# AI Agent Full-Link Communication System Technical Specification (DIDComm V2 Standard)

## 1. Architecture Logic: Doorbell & Parcel Model (Signal & Pull)

To work around FCM payload limitations and ensure **DIDComm V2** end-to-end encryption security, the system adopts a "doorbell and parcel" logic:

* **FCM (Doorbell)**: Only sends a lightweight notification signal telling the phone "you have a new message."
* **HTTPS (Parcel)**: The phone actively pulls the complete encrypted DIDComm Message (JWE) from the server through a secure HTTPS channel.
* **DIDComm (Unpacking)**: Encryption/decryption and signature verification are completed entirely on the phone and MCP/Skill locally; the server cannot decrypt the message body.

> **Asymmetric Transport Path Note**:
> - **Downlink** (Phone -> MCP/Skill): The phone submits commands via HTTPS (power-saving, no persistent connection needed), and the server forwards them to MCP/Skill via WebSocket (MCP/Skill is always online).
> - **Uplink -- International Users** (MCP/Skill -> Phone): MCP/Skill reports results via WebSocket, the server notifies the phone via FCM signal, and the phone pulls via HTTPS (avoids battery drain from maintaining a persistent connection).
> - **Uplink -- China Users** (MCP/Skill -> Phone): MCP/Skill reports results via WebSocket, the server detects the user's `push_channel=websocket`, and if WS is online, pushes the JWE directly; if offline, stages the message, and the phone pulls it via Message Pickup 3.0 after reconnection.

---

## 2. System Topology Diagram

```
┌─────────────────────────────────────────────────────────────────────────┐
│                                                                         │
│    Phone (Flutter App)              Server (Mediator)      MCP/Skill    │
│                                                                         │
│    ┌──────────────┐              ┌──────────────┐           ┌────────┐ │
│    │ DIDComm      │   HTTPS      │ Mediator     │ WebSocket │MCP/    │ │
│    │ Pack/Unpack  │──────────────│              │───────────│Skill   │ │
│    │              │<─────────────│ - Message    │<──────────│DIDComm │ │
│    │ FCM Listener │              │   staging    │           │        │ │
│    └──────────────┘              │ - FCM push   │           └────────┘ │
│         ↑                        │ - WS routing │                      │
│         │ FCM Signal             │              │                      │
│    ┌──────────────┐              │ - WS direct  │                      │
│    │ FCM / APNs   │<─────────────── FCM push (international users) ─── │
│    └──────────────┘              │   push*      │                      │
│         ↑                        └──────────────┘                      │
│         │ WS direct push (China users)*                               │
│    ┌──────────────┐                                                    │
│    │ WS Listener  │<──── WS online direct push JWE / offline staging   │
│    └──────────────┘       Pickup ──────────────────────────────────    │
│                                                                         │
│    * China users: push_channel=websocket, no FCM                        │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘

Message flow:

  Downlink (Phone -> MCP/Skill):              Uplink (MCP/Skill -> Phone) -- International:
    Flutter -> HTTPS POST -> Mediator         MCP/Skill -> WebSocket Send -> Mediator
    -> WebSocket Forward -> MCP/Skill         -> Redis staging -> FCM Signal
    -> DIDComm Unpack -> Execute              -> HTTPS GET -> Flutter
                                              -> DIDComm Unpack -> UI update

                                             Uplink (MCP/Skill -> Phone) -- China:
                                              MCP/Skill -> WebSocket Send -> Mediator
                                              -> Check push_channel=websocket
                                              -> WS online: direct push JWE -> Flutter
                                              -> WS offline: staging -> reconnect Pickup pull
                                              -> DIDComm Unpack -> UI update
```

---

## 3. Identity and Key Management

### 3.1 Identity Model

The system uses **DID** as identity identifiers. Each participant has a unique DID and associated key pair:

| Role | Identity | Key Usage |
|:-----|:---------|:----------|
| Phone (User) | `did:ignite:user_{uuid}` | Sign commands, decrypt messages from MCP/Skill |
| MCP/Skill | `did:ignite:agent_{uuid}` | Sign feedback, decrypt commands from phone |
| Server (Mediator) | `did:ignite:mediator` | Does not participate in DIDComm encryption/decryption, only routes and forwards |

### 3.2 Key Exchange and Trust Establishment

Before first use, the following binding process must be completed:

```
Phone (Flutter)                        Server (Mediator)              MCP/Skill
    │                                       │                              │
    │  1. User registration/login            │                              │
    │  (Generate DID + Ed25519 key pair,     │                              │
    │   store private key in                 │                              │
    │   flutter_secure_storage)              │                              │
    │ ──────────────────────────────────────>│                              │
    │  { did: "did:ignite:user_xxx",          │                              │
    │    public_key: <Ed25519 pubkey> }        │                              │
    │                                       │                              │
    │  2. Bind MCP/Skill                     │                              │
    │ ──────────────────────────────────────>│                              │
    │  { agent_id: "did:ignite:agent_yyy" }  │                              │
    │                                       │                              │
    │                                       │  3. Server verifies binding  │
    │                                       │     (whether user has        │
    │                                       │      control over this       │
    │                                       │      MCP/Skill)              │
    │                                       │                              │
    │  4. Return MCP/Skill DID document      │                              │
    │ <──────────────────────────────────────│                              │
    │  { did: "did:ignite:agent_yyy",         │                              │
    │    public_key: <MCP/Skill Ed25519 pubkey>, │                           │
    │    ws_endpoint: "wss://..." }            │                              │
    │                                       │                              │
    │  5. Phone locally caches MCP/Skill     │                              │
    │     public key                         │                              │
    │     (used for subsequent DIDComm       │                              │
    │      encryption)                       │                              │
```

> **Key Rotation**: When either party rotates keys, the counterpart is notified via DID document update. The server maintains a cache of the latest DID documents. MCP/Skill performs a DIDComm Mediator handshake via WebSocket at startup to register its latest keys.

---

## 4. Full-Link Interaction Details

### 4.1 Downlink: Phone Controls MCP/Skill

1.  **Message Encapsulation (Flutter)**: Uses the locally stored private key to sign the command, and encrypts it against MCP/Skill's public key, generating a DIDComm Encrypted Message (JWE).
2.  **Command Submission (HTTPS)**: Submits the JWE to the server via `POST /v1/agents/{agent_id}/command`. The `agent_id` is specified in the URL path.
3.  **Authentication & Routing (Server)**: The server verifies that the user associated with the Bearer Token is authorized to send commands to that `agent_id`. After verification, the JWE is forwarded to MCP/Skill via WebSocket.
4.  **Execution (MCP/Skill)**: The MCP/Skill terminal receives the message, decrypts and verifies the signature locally, then executes it.

### 4.2 Uplink: MCP/Skill Reports to Phone (Core Reliable Solution)

1.  **Result Encapsulation (MCP/Skill)**: When MCP/Skill encounters an X402 payment challenge, it constructs a `payment-auth-request` encrypted JWE (encrypted with the user's public key), containing payment_id, merchant_did, amount, description, etc.
2.  **Data Reporting (WebSocket)**: MCP/Skill sends the JWE to the Mediator via WebSocket.
3.  **Staging & Signaling (Server)**:
    * The server receives and stores the message in cache (Redis/DB), generating a `msg_id`.
    * The server looks up the bound user by `agent_id` and verifies message ownership.
    * **International users** (`push_channel: "fcm"`): Calls **FCM** to send a Data Message: `{"type": "SIGNAL", "msg_id": "uuid-123"}`.
    * **China users** (`push_channel: "websocket"`):
        - WS online: Directly pushes the JWE to the phone via WebSocket.
        - WS offline: Stages the message; the phone pulls it via Message Pickup 3.0 protocol (`status-request` / `batch-pickup`) after reconnection.
4.  **Active Pull (Flutter)** -- International FCM channel only:
    * Flutter receives the FCM signal and sends a request: `GET /v1/sync/messages/uuid-123`.
5.  **Decrypt & Display (Flutter)**: After receiving the JWE, performs DIDComm `Unpack` in a local Isolate, and upon successful verification displays the payment authorization interface (amount, merchant, description).
6.  **User Authorization (Flutter)**: After the user taps "Authorize", the Flutter App creates a Session Key:
    * Generates an Ed25519 ephemeral key pair.
    * Submits an on-chain transaction to register the Session Key (binding owner, spending_limit, scopes, expires_at).
    * After on-chain confirmation, constructs a `payment-auth-response` encrypted JWE (containing `session_key_pubkey`, `session_key_tx_signature`, etc.).
7.  **Authorization Return (Flutter -> MCP/Skill)**: Flutter submits the encrypted response via HTTPS to the Mediator, and the Mediator forwards it to MCP/Skill via WebSocket.
8.  **On-Chain Payment (MCP/Skill)**: MCP/Skill uses the received Session Key to execute the on-chain payment on behalf of the user (signs an ExecutePayment transaction), without needing to request user authorization again.

---

## 5. Key Interfaces and Protocol Definitions

### 5.1 Dual-Layer Authentication

The system uses two independent authentication mechanisms, each serving its own purpose:

| Layer | Mechanism | Purpose | Verifier |
|:------|:----------|:--------|:---------|
| **Transport Layer** | HTTPS Bearer Token (JWT) | Verifies "who is calling the API" -- ensures only authorized users can pull/submit messages | Server (Mediator) |
| **Message Layer** | DIDComm signature (`Unpack`'s `authenticated`) | Verifies "who sent the message" -- confirms the message was indeed sent by the bound MCP/Skill/user | Client (Flutter/MCP/Skill) |

The Bearer Token is a JWT issued by the server after user login, containing a `user_did` field. The Token is bound to the DID identity: the server uses the `user_did` in the Token to look up the user's bound MCP/Skill list for authorization.

### 5.2 Command Submission Interface (Downlink)

* **Endpoint**: `POST /v1/agents/{agent_id}/command`
* **Header**: `Authorization: Bearer <token>`
* **Request Body**:
    ```json
    {
      "jwe_envelope": "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9..."
    }
    ```
* **Authentication**: The server verifies the binding between the `user_did` in the Token and the `agent_id`.

### 5.3 Message Pull Interface (Uplink)

* **Endpoint**: `GET /v1/sync/messages/{msg_id}`
* **Header**: `Authorization: Bearer <token>`
* **Authentication**: The server verifies that the `user_did` in the Token matches the message owner, preventing unauthorized access to other users' messages. MCP/Skill uses DIDComm key authentication when connecting via WebSocket.
* **Response**:
    ```json
    {
      "msg_id": "uuid-123",
      "jwe_envelope": "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9...",
      "created_at": 1712739265
    }
    ```

### 5.4 Batch Sync Mechanism (Fallback)

To prevent FCM signal loss or missed messages during WS offline periods, the App must call this when **returning to the foreground**:

* **Endpoint**: `GET /v1/sync/list?after={last_read_id}&limit=100`
* **Purpose**: Returns all unread messages after `last_read_id`.
* **Response**:
    ```json
    {
      "messages": [
        {"msg_id": "uuid-124", "jwe_envelope": "...", "created_at": 1712739270},
        {"msg_id": "uuid-125", "jwe_envelope": "...", "created_at": 1712739280}
      ],
      "has_more": false
    }
    ```

> **Full Sync**: If the App loses local data (reinstall/device switch), use `GET /v1/sync/list?after=&limit=100` (omit the `after` parameter) to sync from the earliest message. The server filters by `user_did` to ensure only that user's messages are returned.

---

## 6. Development Implementation Guide

### 6.1 Phone (Flutter)

* **Secure Storage**: Private keys must be stored in `flutter_secure_storage` (Android uses KeyStore, iOS uses Keychain).
* **Encryption/Decryption Performance**: Must use **Rust FFI** for DIDComm logic. Call it using a `Worker Isolate` in Flutter to avoid blocking the main thread and causing animation frame drops.
* **Push Listener**:
    * **International users**: Must handle both `FirebaseMessaging.onMessage` (foreground) and `FirebaseMessaging.onBackgroundMessage` (background).
    * **China users**: Maintain a persistent WebSocket connection with the Mediator, listen on `onWebSocketMessage` for real-time JWE reception. When the App goes to background, WS may disconnect; upon returning to foreground, auto-reconnect and execute Message Pickup (`status-request` / `batch-pickup`) to pull staged offline messages.

> **iOS Note**: On iOS, FCM relies on APNs. When the App is force-quit, push notifications may be delayed or undeliverable. The batch sync mechanism in 5.4 is a necessary fallback for iOS -- the App must trigger a sync when returning to the foreground to ensure no messages are missed.

### 6.2 Server (Mediator)

* **Cache Strategy**: Redis is recommended for JWE cache storage, with a **7-day TTL**.
* **Push Optimization**: Configure FCM `Priority` for different message types. Use `High` for control command feedback, `Normal` for regular status updates.
* **WebSocket Authentication**:
    - MCP/Skill uses DIDComm Agent key signatures for WebSocket connection authentication.
    - Upon connection establishment, perform DIDComm Mediator protocol handshake (`coordinate-mediation/2.0/mediate-request` -> `mediate-grant` -> `keylist-update`).
    - The server (Mediator) routes messages based on DIDs registered during handshake, ensuring messages are only sent to authenticated connections.

### 6.3 MCP/Skill Terminal

* **WebSocket Protocol**: Uses standard **DIDComm V2** WebSocket connection, following the `coordinate-mediation/2.0` protocol handshake (consistent with `ignite-pay-did.md` section 4.1).
* **Reconnection Mechanism**: After WebSocket disconnection, auto-reconnect every 3 seconds and re-execute the full handshake (`mediate-request` -> `mediate-grant` -> `keylist-update` -> `peer-did-discovery`).
* **Offline Messages**: After reconnection, pull offline messages via Message Pickup 3.0 protocol (`status-request` / `batch-pickup`). Offline messages older than 7 days are recovered via `GET /v1/sync/list`.

---

## 7. Security and Verification Specifications

| Verification Step | Implementation | Purpose |
| :--- | :--- | :--- |
| **Message Source Verification** | `authenticated` property in DIDComm `Unpack` result | Confirms the message was indeed sent by the bound MCP/Skill. |
| **Anti-Replay Verification** | Record and check DIDComm Message `id` (Unique Message ID) | Prevents malicious interception and resubmission of messages. |
| **Timeliness Verification** | Check `expires_time` (Expiration) in message body | Discards stale commands caused by extreme network delays. |
| **Transport Encryption** | Full-link TLS 1.3 | Protects outer metadata, combined with DIDComm for dual-layer encryption. |
| **API Authentication** | HTTPS Bearer Token (JWT) + User-MCP/Skill binding verification | Prevents unauthorized access to other users' messages. |
| **WebSocket Authentication** | DIDComm key signature + Mediator protocol handshake | Prevents impersonation of MCP/Skill connections or message eavesdropping. |

---

## 8. WebSocket Message Routing

MCP/Skill establishes persistent WebSocket connections via the DIDComm Mediator protocol (`coordinate-mediation/2.0`). Message routing is based on DIDs rather than Topics:

| Direction | Message Type | DIDComm Protocol | Description |
|:----------|:-------------|:-----------------|:------------|
| MCP/Skill -> Mediator | `mediate-request` | Coordinate Mediation 2.0 | Register as Mediator client |
| MCP/Skill -> Mediator | `keylist-update` | Coordinate Mediation 2.0 | Register receiving key `{did}#key-1` |
| MCP/Skill -> Mediator | `forward` | Routing 2.0 | Forward DIDComm message to target |
| Mediator -> MCP/Skill | JWE Message | DIDComm V2 | Deliver encrypted message |
| Phone (China) -> Mediator | `mediate-request` | Coordinate Mediation 2.0 | China user WS connection registration |
| Phone (China) -> Mediator | `keylist-update` | Coordinate Mediation 2.0 | Register user DID receiving key |
| Mediator -> Phone (China) | JWE Message | DIDComm V2 | WS online direct push encrypted message |
| Phone (China) -> Mediator | `status-request` | Message Pickup 3.0 | Query count of staged offline messages |
| Mediator -> Phone (China) | `status` | Message Pickup 3.0 | Return staged message count |
| Phone (China) -> Mediator | `batch-pickup` | Message Pickup 3.0 | Batch pull offline staged messages |
| Mediator -> Phone (China) | `batch` | Message Pickup 3.0 | Return batch messages |

> **Routing Mechanism**: The Mediator looks up registered WebSocket connections by the message target's DID and delivers the message to the corresponding connection. Messages for offline clients are staged in Redis (7-day TTL); clients pull them via Message Pickup 3.0 protocol after reconnection.

---

## 9. China User WebSocket Channel

### 9.1 Background

Users in mainland China cannot use Google FCM, so the current "FCM doorbell + HTTPS pull" model is not applicable. A pure WebSocket channel is needed for China users: the phone maintains a persistent WS connection with the Mediator, and messages are pushed directly via WS in real time.

### 9.2 Dual-Channel Architecture

```
International users: MCP -> WS -> Mediator -> FCM signal -> Phone HTTPS pull (current)
China users:         MCP -> WS -> Mediator -> WS direct push -> Phone real-time receive (new)
```

### 9.3 Identifying China Users

The phone determines China user status at registration via the following signals (any match counts as a China user):
1. `Locale` contains `zh_CN`, or language is `zh` and country is `CN`
2. Timezone is `Asia/Shanghai`, `Asia/Chongqing`, etc.

### 9.4 Registration Flow Differences

- **China users**: Register `push_channel: "websocket"`, do not register FCM token
- **International users**: Register `push_channel: "fcm"` + FCM token (unchanged)

### 9.5 Routing Strategy

When pushing a message to the phone:
1. Query the user's `push_channel` preference
2. `"websocket"`: Check if the user's WS session is online
   - Online: Directly push JWE via WS
   - Offline: Store in message queue (phone pulls via Pickup protocol after reconnection)
3. `"fcm"`: Use existing FCM signal + HTTPS pull logic

---

## 10. Next Step Action Items

1.  **Integrate Firebase**: Complete Flutter and FCM basic integration.
2.  **Write FFI Bridge**: Wrap Rust `didcomm-rs` into a Flutter-usable library.
3.  **Implement WebSocket Mediator**: Deploy a WebSocket server supporting DIDComm Coordinate Mediation 2.0, configure TLS and DID key authentication (consistent with `ignite-pay-did.md` section 4.1 handshake protocol).
4.  **Implement Mediator API**: Include three core interfaces -- command submission (HTTPS), message staging (Redis), and batch sync.
5.  **Key Management Flow**: Implement DID generation during user registration, MCP/Skill binding, and key rotation notification mechanism.
