In the DIDComm V2 architecture, binding between an MCP (Model Context Protocol) server or Agent Skill and the mobile device essentially means **establishing a "peer-to-peer" trusted relationship (Connection/Relationship)**.

To implement "DID binding" and route messages through "FCM notifications," you need to complete three core steps: **identity exchange, route mapping, and credential reporting**.

---

## 1. DID Binding Flow: Handshake and Connection Establishment

Since DIDs are decentralized, the MCP/Skill cannot know the mobile device's address in advance. The **DIDComm Out-of-Band (OOB)** protocol is typically used for binding:

### Step 1: MCP/Skill Generates an Invitation
1. The MCP server generates a "Connection Invitation" containing its own DID.
2. The invitation is converted into a QR code or a Deep Link.

### Step 2: Mobile Device Scans/Parses
1. The mobile Flutter App scans the QR code and obtains the MCP's DID.
2. **Bidirectional binding**: The mobile device generates a `Connection Request` message containing its own DID.
3. **Key action**: The mobile device includes its **FCM Token** in the message's `decorator` or a custom field.

### Step 3: MCP Stores the Relationship Mapping
After receiving the request, the MCP establishes a mapping table in its local database:
| Mobile DID | Mobile FCM Token | Trust Status |
| :--- | :--- | :--- |
| `did:peer:user_123` | `fcm_token_abc...` | Verified |



---

## 2. Message Notification: How to Let FCM Find the Correct DID

When a Skill in the MCP generates a message that needs to be pushed to the mobile device, the flow is as follows:

### 1. Business Trigger
The Skill generates an encrypted message (JWM) targeting `did:peer:user_123`.

### 2. Route Lookup (Mediator Logic)
The server-side logic checks the push credentials associated with that DID:
* Finds that `did:peer:user_123` is bound to `fcm_token_abc`.
* Detects that this user belongs to the "China region" and automatically switches to **JPush (Jiguang)** logic (as set in your previous test plan).

### 3. Send Signal
The server sends a Data Message to FCM. This message does not contain DIDComm plaintext, only an index:
```json
{
  "to": "fcm_token_abc",
  "data": {
    "type": "DIDCOMM_ARRIVAL",
    "msg_id": "storage_uuid_001", // The ID where the server temporarily stores this encrypted message
    "sender_did": "did:peer:mcp_skill_789"
  }
}
```

---

## 3. Technical Implementation Details (Development Guide)

### A. Mobile Device: How to Report the FCM Token?
In Flutter, it is recommended to encapsulate the FCM Token update as a specific DIDComm message type (e.g., `https://didcomm.org/push-notifications/1.0/set-info`):

```dart
// Pseudocode: Send a DIDComm message to the MCP to update push notification info
var pushUpdateMsg = Message(
  type: "https://didcomm.org/push-notifications/1.0/set-info",
  from: myDid,
  to: mcpDid,
  body: {
    "fcm_token": currentFcmToken,
    "platform": "android",
    "region": "CN" // Used for switching to Jiguang for domestic testing
  }
);
// Encrypt and send to MCP using Authcrypt
```

### B. Server Side (MCP/Skill): How to Process?
When a Skill needs to push a notification, it does not call FCM directly. Instead, it calls a **Message Dispatch Component (Mediator Service)**:

1. **Cache the message**: Store the encrypted JWM in Redis with the key set to `msg_id`.
2. **Query the binding**: Look up the corresponding `fcm_token` via the target DID.
3. **Push**: Call the Firebase Admin SDK to send a `data`-type push notification.

---

## 4. Key Point: "Dynamic Binding" for Domestic Testing

When testing JPush (Jiguang) domestically, the logic remains the same:
1. The mobile device reports JPush's **RegistrationID (RID)** instead of an FCM Token.
2. When pushing, the server detects that the DID is bound to an RID, and calls JPush's REST API to send a custom message.

---

## 5. Summary: The Binding and Notification Closed Loop



* **Binding (Identity Binding)**: Exchange DIDs via the OOB protocol, and report the **FCM Token/JPush ID** as an attribute (or associated data) of the DID to the MCP.
* **Notification**: MCP generates a message -> queries the mapping table to obtain the Token -> sends an FCM signal -> the mobile device pulls the data back.

**Architecture Recommendation:**
You can implement a simple **DID-to-Push-Channel** mapping service on the MCP server side. This way, no matter how many Skills you add in the future, they only need to know the target DID, and the "how to notify the phone" part is handled entirely by this mapping service.
