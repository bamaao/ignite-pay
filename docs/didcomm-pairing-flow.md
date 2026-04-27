# DIDComm 配对流程

本文档记录 ignite-pay-mcp / ignite-pay-merchant-mcp 与手机 App 首次建立 DID 连接的配对流程。用户端和商户端采用相同的配对协议。

## 1. 统一配对流程

### 1.1 时序图

```
MCP Server                      DIDComm Mediator              Phone App
(ignite-pay-mcp                 (user: 8080)                  (ignite_pay_app
 或 merchant-mcp)               (merchant: 4000)              或 merchant app)
   │                                  │                              │
   │─── WS connect ──────────────────>│                              │
   │<── ws-challenge (nonce) ─────────│                              │
   │─── ws-challenge-response ───────>│  Ed25519签名 + DID Document  │
   │<── ws-auth-ok ───────────────────│                              │
   │─── mediate-request ─────────────>│                              │
   │<── mediate-grant ────────────────│                              │
   │─── keylist-update ──────────────>│  注册路由键                   │
   │<── keylist-update-response ──────│                              │
   │─── peer-introduction ───────────>│  共享 DID Document           │
   │                                  │                              │
   │ [生成 QR: didcomm://?_oob=<b64>] │                              │
   │                                  │                   [扫描 QR]  │
   │                                  │                              │
   │                                  │<──── WS connect ────────────│
   │                                  │───── ws-challenge ──────────>│
   │                                  │<──── ws-challenge-response ─│
   │                                  │───── ws-auth-ok ────────────>│
   │                                  │<──── mediate-request ───────│
   │                                  │───── mediate-grant ─────────>│
   │                                  │<──── keylist-update ────────│
   │                                  │───── keylist-update-res ────>│
   │                                  │<──── peer-introduction ─────│
   │                                  │                              │
   │                                  │<──── connection-request ────│
   │<── connection-request ──────────│  (mediator 路由)             │
   │                                  │                              │
   │ [存储 phone_did, 注册 peer]      │                              │
   │═══ 配对完成 ═══                   │                              │
```

### 1.2 步骤详解

#### Step 1: MCP 启动连接 Mediator

MCP 启动时 `MediatorConnection::connect_and_run()` 生成 `did:ignite:z<Base58(Ed25519_pubkey)>` 身份，通过 WS 连接 mediator。

**认证握手 (Phase 0)**：

| 方向 | 消息类型 | 说明 |
|------|---------|------|
| Mediator → MCP | `ws-challenge` | 发送 nonce |
| MCP → Mediator | `ws-challenge-response` | Ed25519 签名 nonce + DID Document |
| Mediator → MCP | `ws-auth-ok` | 认证成功 |

**Mediation 握手 (Phase A)**：

| 方向 | 消息类型 | 协议 |
|------|---------|------|
| MCP → Mediator | `mediate-request` | `coordinate-mediation/2.0` |
| Mediator → MCP | `mediate-grant` | |
| MCP → Mediator | `keylist-update` | 注册 DID 路由键 |
| MCP → Mediator | `peer-introduction` | `peer-did-discovery/1.0`，共享 DID Document |

#### Step 2: MCP 生成 OOB 邀请 (QR 码)

- 启动时若无已配对手机，自动生成并打印 QR 码
- 或通过 MCP 工具 `generate_pairing_invitation` 按需生成

QR 内容：`didcomm://?_oob=<base64url(JSON)>`

OOB Invitation 消息结构：

```json
{
  "type": "https://didcomm.org/out-of-band/2.0/invitation",
  "from": "did:ignite:z...",
  "body": {
    "label": "Ignite Pay MCP",
    "goal_code": "p2p-messaging",
    "accept": ["didcomm/v2"],
    "did_document": {
      "@context": "https://www.w3.org/ns/did/v1",
      "id": "did:ignite:z...",
      "verificationMethod": [{
        "id": "did:ignite:z...#key-1",
        "type": "Ed25519VerificationKey2020",
        "publicKeyBase58": "..."
      }],
      "keyAgreement": [{
        "id": "did:ignite:z...#key-agreement-1",
        "type": "X25519KeyAgreementKey2020",
        "publicKeyBase58": "..."
      }]
    },
    "services": [{
      "id": "#mediator",
      "type": "did-communication",
      "service_endpoint": "ws://...",
      "routing_keys": ["did:ignite:z..."]
    }]
  }
}
```

关键点：邀请中内嵌完整 DID Document（含 X25519 keyAgreement），App 无需额外 DID 解析即可加密通信。

#### Step 3: App 扫码并连接 Mediator

App 检测 `didcomm://` 前缀 → 解析 OOB invitation：
1. Base64url 解码 `_oob` 参数
2. 提取 MCP DID、mediator WS 地址、DID Document
3. 连接 mediator（执行同样的 ws-challenge + coordinate-mediation 握手）

#### Step 4: App 发送 connection-request

```json
{
  "type": "https://didcomm.org/ignite-pay/1.0/connection-request",
  "from": "did:ignite:z<app>",
  "to": ["did:ignite:z<mcp>"],
  "body": {
    "push_channel": "websocket",
    "fcm_token": "..."
  }
}
```

通过 Mediator 路由到 MCP（可明文或 JWE 加密）。

#### Step 5: MCP 完成配对

`mediator.rs` 中 `handle_incoming_message()` 检测到 `connection-request`：
1. 提取 App DID
2. 若携带 DID Document，注册为加密 peer
3. 持久化 DID 到 sled（`__paired_phone__` 键）

配对完成后，MCP 即可向 App 发送加密的 `payment-auth-request`。

---

## 2. 两端差异

### 2.1 服务端口

| 服务 | 默认端口 | 说明 |
|------|---------|------|
| didcomm-router (user) | 8080 | 用户端 Mediator |
| didcomm-router (merchant) | 4000 | 商户端 Mediator |
| ignite-pay-hub-registry | 3004 | Hub 注册服务 |

### 2.2 服务端 (MCP) 实现状态

| 组件 | ignite-pay-mcp | ignite-pay-merchant-mcp |
|------|---------------|------------------------|
| WS challenge-response 认证 | 有 | 有（简化版） |
| Coordinate-mediation 握手 | 有 | 有 |
| OOB invitation 生成 | 有 | 有 |
| QR 码生成 | 有 | 有 |
| connection-request 处理 | 有 | 有 |
| 启动时自动打印 QR | 有 | 有 |

### 2.3 客户端 (App) 实现状态

| 组件 | ignite_pay_app | ignite_pay_merchant_app |
|------|---------------|------------------------|
| QR 扫描（mobile_scanner） | 已实现 | 待实现 |
| `didcomm://` OOB 解析 | 已实现 | 待实现 |
| WS challenge-response 认证 | 已实现 | 待实现 |
| coordinate-mediation 握手 | 已实现 | 待实现 |
| connection-request 发送 | 已实现 | 待实现 |
| payment-auth-request 接收 | 已实现 | 待实现 |

商户 MCP 服务端已完整支持 OOB + connection-request 配对协议，商户 App 客户端需要补充 QR 扫描和 DIDComm 配对的客户端实现。

---

## 3. DIDComm 消息类型汇总

### 3.1 配对相关

| 消息类型 | 方向 | 说明 |
|---------|------|------|
| `out-of-band/2.0/invitation` | MCP → App (via QR) | OOB 邀请，含 DID、DID Doc、mediator 地址 |
| `ignite-pay/1.0/connection-request` | App → MCP | 配对请求，含 push_channel 偏好 |

### 3.2 Mediator 握手

| 消息类型 | 协议 | 说明 |
|---------|------|------|
| `ignite-pay/1.0/ws-challenge` | 自定义 | WS 认证挑战（含 nonce） |
| `ignite-pay/1.0/ws-challenge-response` | 自定义 | Ed25519 签名 + DID Document |
| `ignite-pay/1.0/ws-auth-ok` | 自定义 | 认证成功确认 |
| `coordinate-mediation/2.0/mediate-request` | 标准 DIDComm | 请求 mediation |
| `coordinate-mediation/2.0/mediate-grant` | 标准 DIDComm | 授予 mediation |
| `coordinate-mediation/2.0/keylist-update` | 标准 DIDComm | 注册路由键 |
| `peer-did-discovery/1.0/discover` | 自定义 | 共享 DID Document |

### 3.3 消息拾取（离线消息）

| 消息类型 | 协议 | 说明 |
|---------|------|------|
| `messagepickup/3.0/status-request` | 标准 DIDComm | 查询排队消息数量 |
| `messagepickup/3.0/batch-pickup` | 标准 DIDComm | 批量拉取离线消息 |

### 3.4 支付流程（配对后使用）

| 消息类型 | 方向 | 说明 |
|---------|------|------|
| `ignite-pay/1.0/payment-auth-request` | MCP → App | 支付授权请求 |
| `ignite-pay/1.0/payment-auth-response` | App → MCP | 支付授权响应（含 session key） |
| `ignite-pay/1.0/create-channel-request` | Merchant App → MCP | 建立状态通道请求 |
| `ignite-pay/1.0/channel-payment-confirm` | MCP → Merchant App | 通道支付确认 |

---

## 4. 设计特点

- **统一配对协议**：用户端和商户端使用相同的 OOB invitation + connection-request 配对流程
- **非标准 DIDExchange**：使用自定义 `ignite-pay/1.0/connection-request` 而非 Aries DIDExchange 协议
- **DID Document 内嵌邀请**：App 无需额外 DID 解析即可加密通信
- **配对隐式确认**：MCP 存储 App DID 即视为配对成功，不发送 `connection-response`
- **持久化配对**：App DID 存储在 sled `__paired_phone__` 键，重启后自动恢复

---

## 5. 关键文件

### 5.1 共享协议库

| 文件 | 职责 |
|------|------|
| `ignite-pay-core/src/didcomm.rs` | DIDComm 消息构建、OOB invitation、pack/unpack |
| `ignite-pay-core/src/identity.rs` | `did:ignite` 生成、DID Document 构建 |

### 5.2 用户端

| 文件 | 职责 |
|------|------|
| `ignite-pay-mcp/src/mediator.rs` | MCP mediator 连接、OOB 邀请生成、入站消息处理 |
| `ignite-pay-mcp/src/main.rs` | `generate_pairing_invitation` 工具、启动时自动 QR |
| `ignite_pay_app/lib/qr_scanner_screen.dart` | QR 扫描 UI |
| `ignite_pay_app/lib/services/didcomm_service.dart` | `parseInvitationAndConnect()`、mediator 连接 |
| `ignite_pay_app/rust/src/api/ws_client.rs` | Rust WS 客户端：mediator 握手 |
| `ignite_pay_app/rust/src/api/simple.rs` | `parse_oob_invitation()`、`send_connection_request()` |

### 5.3 商户端

| 文件 | 职责 |
|------|------|
| `ignite-pay-merchant-mcp/src/mediator.rs` | 商户 MCP mediator 连接、OOB 邀请生成 |
| `ignite-pay-merchant-mcp/src/main.rs` | 启动时自动生成 QR |
| `ignite_pay_merchant_app/rust/src/api/merchant_didcomm.rs` | 商户 App DIDComm 通信 |
