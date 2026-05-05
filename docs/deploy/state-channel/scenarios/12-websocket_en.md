# Scenario 12: WebSocket Real-time Communication

## 1. Scenario Description

Channel services communicate in real time via WebSocket using bidirectional messaging. After establishing a connection, Ed25519 signature authentication is required, after which LeafUpdates, co-signing requests, HTLC status changes, and other messages can be pushed in real time.

## 2. Participants

| Role | Responsibility |
|:-----|:-----|
| All roles | Connect to peer WebSocket, authenticate, then send and receive real-time messages |

## 3. Prerequisites

- Peer service is deployed and accessible
- Peer WebSocket address is known (`ws://host:port/ws`)
- A valid Ed25519 key pair is available

## 4. Operation Flow

```
Client                                  Server
 │  1. Connect ws://host:port/ws         │
 │───────────────────────────────────────→│
 │                                        │
 │  2. Send authentication message        │
 │  {"type": "auth",                      │
 │   "pubkey": "<base58>",                │
 │   "signature": [64 bytes],             │
 │   "timestamp": 1234567890}             │
 │───────────────────────────────────────→│
 │                                        │  3. Verify signature
 │                                        │  SHA-256("channel-ws-auth:{timestamp}")
 │                                        │  Ed25519.verify(hash, signature, pubkey)
 │                                        │
 │  ← {"type": "auth_ok"}                 │  (Authentication successful)
 │  Or                                    │
 │  ← {"type": "error", "code": 401,      │  (Authentication failed, connection closed)
 │     "message": "authentication failed"}│
 │                                        │
 │  4. Bidirectional message flow          │
 │←──────────────────────────────────────→│
```

## 5. Message Types

### Authentication

| Type | Direction | Fields |
|:-----|:-----|:-----|
| `auth` | Client → Server | `pubkey`, `signature`, `timestamp` |
| `auth_ok` | Server → Client | -- |
| `error` | Bidirectional | `code`, `message` |

### LeafUpdate

| Type | Direction | Fields |
|:-----|:-----|:-----|
| `leaf_update` | Bidirectional | `channel_id`, `sequence`, `leaf_index`, `prev_leaf_hash`, `new_leaf`, `signature` |
| `leaf_update_ack` | Receiver → Sender | `channel_id`, `sequence` |
| `leaf_update_nack` | Receiver → Sender | `channel_id`, `sequence`, `reason` |

### Batch Operations

| Type | Direction | Fields |
|:-----|:-----|:-----|
| `batch_start` | Sender → Receiver | `channel_id`, `count` |
| `batch_item` | Sender → Receiver | `channel_id`, `index`, `update` |
| `batch_commit` | Sender → Receiver | `channel_id` |
| `batch_abort` | Sender → Receiver | `channel_id` |
| `batch_result` | Receiver → Sender | `channel_id`, `applied`, `failed_index` |

### Co-signing

| Type | Direction | Fields |
|:-----|:-----|:-----|
| `cosign_request` | Sender → Receiver | `channel_id`, `sequence`, `root` |
| `cosign_response` | Receiver → Sender | `channel_id`, `sequence`, `cosignature` |

### HTLC

| Type | Direction | Fields |
|:-----|:-----|:-----|
| `htlc_created` | Bidirectional | `channel_id`, `hash_lock`, `amount`, `timelock_slot` |
| `htlc_preimage` | Revealer → Peer | `channel_id`, `hash_lock`, `preimage` |
| `htlc_refunded` | Both parties | `channel_id`, `hash_lock` |

### Multi-hop

| Type | Direction | Fields |
|:-----|:-----|:-----|
| `multihop_init` | Initiator → Hub | `payment_id`, `route`, `hash_lock` |
| `multihop_preimage` | Payee → Hub | `payment_id`, `preimage` |
| `multihop_failed` | Hub → Initiator | `payment_id`, `reason` |

### Settlement

| Type | Direction | Fields |
|:-----|:-----|:-----|
| `challenge_triggered` | Initiator → Peer | `channel_id`, `challenge_slot` |
| `counter_state_submitted` | Peer → Initiator | `channel_id`, `sequence` |
| `settlement_started` | Both parties | `channel_id`, `settle_window` |

### Channel State Change

| Type | Direction | Fields |
|:-----|:-----|:-----|
| `channel_state_changed` | Server → Client | `channel_id`, `new_status` |

## 6. HTTP API Calls

WebSocket connects via `ws://host:port/ws` and does not use the REST API.

JavaScript client example:

```javascript
const ws = new WebSocket('ws://localhost:3001/ws');

// Authentication
ws.onopen = () => {
  const timestamp = Date.now();
  const message = `channel-ws-auth:${timestamp}`;
  // Sign SHA-256 hash of message using Ed25519
  const signature = await ed25519.sign(sha256(message), privateKey);

  ws.send(JSON.stringify({
    type: 'auth',
    pubkey: base58Encode(publicKey),
    signature: Array.from(signature),
    timestamp
  }));
};

ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);
  switch (msg.type) {
    case 'auth_ok':
      console.log('Authentication successful');
      break;
    case 'leaf_update':
      handleLeafUpdate(msg);
      break;
    case 'cosign_response':
      handleCosign(msg);
      break;
  }
};
```

## 7. Rust Library Calls

```rust
use crate::ws::protocol::WsMessage;

// Send message
let msg = WsMessage::LeafUpdate {
    channel_id: hex::encode(channel_id),
    sequence: 5,
    leaf_index: 0,
    prev_leaf_hash: prev_hash.to_vec(),
    new_leaf: serde_json::to_value(&new_leaf)?,
    signature: sig.to_vec(),
};
let text = serde_json::to_string(&msg)?;
ws_sender.send(Message::Text(text.into())).await?;
```

## 8. Error Handling

| Error Code | Cause | Handling |
|:-------|:-----|:-----|
| 400 | Malformed authentication message | Check JSON format |
| 401 | Signature verification failed | Check signature algorithm and timestamp |
| 401 | Business message sent before authentication | Send auth message first |

## 9. Notes

- Authentication signature content: `SHA-256("channel-ws-auth:{timestamp}")`, not directly signing the timestamp string
- Timestamp is used to prevent replay attacks; the server can validate the time difference
- After successful authentication, the peer is registered in `DashMap<pubkey_base58, Sender>`
- The server pushes messages to connected peers via `mpsc::channel`
- On connection disconnect, the peer is automatically removed from `ws_peers`
- All messages use `#[serde(tag = "type")]` tagged JSON format

---

## Related Scenarios

| Scenario | Relationship |
|:-----|:-----|
| [02 Off-chain Payment](02-offchain-payment.md) | `leaf_update` real-time push |
| [03 Batch Pipeline](03-batch-pipeline.md) | `batch_update` batch push |
| [04 HTLC Payment](04-htlc-payment.md) | `htlc_preimage` preimage reveal |
| [05 Cooperative Close](05-cooperative-close.md) | `co_sign_request` co-signing request |
| [06 Dispute Resolution](06-dispute-resolution.md) | `channel_state_change` status change notification |
| [09 Multi-hop Payment](09-multihop-payment.md) | `multihop_relay` cross-hop relay |
