# 场景十二：WebSocket 实时通信

## 1. 场景描述

通道服务之间通过 WebSocket 进行实时双向通信。连接建立后需要 Ed25519 签名认证，之后可实时推送 LeafUpdate、配签请求、HTLC 状态变更等消息。

## 2. 参与角色

| 角色 | 职责 |
|:-----|:-----|
| 所有角色 | 连接对端 WebSocket，认证后收发实时消息 |

## 3. 前置条件

- 对端服务已部署并可访问
- 已知对端 WebSocket 地址 (`ws://host:port/ws`)
- 持有有效的 Ed25519 密钥对

## 4. 操作流程

```
Client                                  Server
 │  1. 连接 ws://host:port/ws             │
 │───────────────────────────────────────→│
 │                                        │
 │  2. 发送认证消息                        │
 │  {"type": "auth",                      │
 │   "pubkey": "<base58>",                │
 │   "signature": [64 bytes],             │
 │   "timestamp": 1234567890}             │
 │───────────────────────────────────────→│
 │                                        │  3. 验证签名
 │                                        │  SHA-256("channel-ws-auth:{timestamp}")
 │                                        │  Ed25519.verify(hash, signature, pubkey)
 │                                        │
 │  ← {"type": "auth_ok"}                 │  (认证成功)
 │  或                                    │
 │  ← {"type": "error", "code": 401,      │  (认证失败，连接关闭)
 │     "message": "authentication failed"}│
 │                                        │
 │  4. 双向消息流                          │
 │←──────────────────────────────────────→│
```

## 5. 消息类型

### 认证

| 类型 | 方向 | 字段 |
|:-----|:-----|:-----|
| `auth` | Client → Server | `pubkey`, `signature`, `timestamp` |
| `auth_ok` | Server → Client | — |
| `error` | 双向 | `code`, `message` |

### LeafUpdate

| 类型 | 方向 | 字段 |
|:-----|:-----|:-----|
| `leaf_update` | 双向 | `channel_id`, `sequence`, `leaf_index`, `prev_leaf_hash`, `new_leaf`, `signature` |
| `leaf_update_ack` | 接收方 → 发送方 | `channel_id`, `sequence` |
| `leaf_update_nack` | 接收方 → 发送方 | `channel_id`, `sequence`, `reason` |

### 批量操作

| 类型 | 方向 | 字段 |
|:-----|:-----|:-----|
| `batch_start` | 发送方 → 接收方 | `channel_id`, `count` |
| `batch_item` | 发送方 → 接收方 | `channel_id`, `index`, `update` |
| `batch_commit` | 发送方 → 接收方 | `channel_id` |
| `batch_abort` | 发送方 → 接收方 | `channel_id` |
| `batch_result` | 接收方 → 发送方 | `channel_id`, `applied`, `failed_index` |

### 配签

| 类型 | 方向 | 字段 |
|:-----|:-----|:-----|
| `cosign_request` | 发送方 → 接收方 | `channel_id`, `sequence`, `root` |
| `cosign_response` | 接收方 → 发送方 | `channel_id`, `sequence`, `cosignature` |

### HTLC

| 类型 | 方向 | 字段 |
|:-----|:-----|:-----|
| `htlc_created` | 双向 | `channel_id`, `hash_lock`, `amount`, `timelock_slot` |
| `htlc_preimage` | 揭示方 → 对方 | `channel_id`, `hash_lock`, `preimage` |
| `htlc_refunded` | 双方 | `channel_id`, `hash_lock` |

### 多跳

| 类型 | 方向 | 字段 |
|:-----|:-----|:-----|
| `multihop_init` | 发起方 → Hub | `payment_id`, `route`, `hash_lock` |
| `multihop_preimage` | 收款方 → Hub | `payment_id`, `preimage` |
| `multihop_failed` | Hub → 发起方 | `payment_id`, `reason` |

### 结算

| 类型 | 方向 | 字段 |
|:-----|:-----|:-----|
| `challenge_triggered` | 发起方 → 对方 | `channel_id`, `challenge_slot` |
| `counter_state_submitted` | 对方 → 发起方 | `channel_id`, `sequence` |
| `settlement_started` | 双方 | `channel_id`, `settle_window` |

### 通道状态变更

| 类型 | 方向 | 字段 |
|:-----|:-----|:-----|
| `channel_state_changed` | Server → Client | `channel_id`, `new_status` |

## 6. HTTP API 调用

WebSocket 通过 `ws://host:port/ws` 连接，不走 REST API。

JavaScript 客户端示例：

```javascript
const ws = new WebSocket('ws://localhost:3001/ws');

// 认证
ws.onopen = () => {
  const timestamp = Date.now();
  const message = `channel-ws-auth:${timestamp}`;
  // 使用 Ed25519 签名 message 的 SHA-256 hash
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
      console.log('认证成功');
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

## 7. Rust 库调用

```rust
use crate::ws::protocol::WsMessage;

// 发送消息
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

## 8. 错误处理

| 错误码 | 原因 | 处理 |
|:-------|:-----|:-----|
| 400 | 认证消息格式错误 | 检查 JSON 格式 |
| 401 | 签名验证失败 | 检查签名算法和时间戳 |
| 401 | 未认证就发送业务消息 | 先发送 auth 消息 |

## 9. 注意事项

- 认证签名内容：`SHA-256("channel-ws-auth:{timestamp}")`，不是直接签时间戳字符串
- 时间戳用于防重放攻击，服务端可校验时间差
- 认证成功后 peer 注册在 `DashMap<pubkey_base58, Sender>`
- 服务端通过 `mpsc::channel` 向连接的 peer 推送消息
- 连接断开时自动从 `ws_peers` 移除
- 所有消息使用 `#[serde(tag = "type")]` tagged JSON 格式

---

## 相关场景

| 场景 | 关系 |
|:-----|:-----|
| [02 离链支付](02-offchain-payment.md) | `leaf_update` 实时推送 |
| [03 批量 Pipeline](03-batch-pipeline.md) | `batch_update` 批量推送 |
| [04 HTLC 支付](04-htlc-payment.md) | `htlc_preimage` 原像揭示 |
| [05 协作关闭](05-cooperative-close.md) | `co_sign_request` 配签请求 |
| [06 争议解决](06-dispute-resolution.md) | `channel_state_change` 状态变更通知 |
| [09 多跳支付](09-multihop-payment.md) | `multihop_relay` 跨跳中继 |
