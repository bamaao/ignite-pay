# DIDComm 配对流程

本文档记录 ignite-pay-mcp / ignite-pay-merchant-mcp 与手机 App 首次建立 DID 连接的配对流程。用户端和商户端采用相同的配对协议。

## 1. 统一配对流程

### 1.1 时序图

```
MCP Server                      MCP's Mediator              Phone App
(ignite-pay-mcp                                              (ignite_pay_app
 或 merchant-mcp)              Phone's Mediator             或 merchant app)
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
   │  QR 中含 HTTP URL (非 WSS)       │                              │
   │                                  │                   [扫描 QR]  │
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
   │                                  │                   包含 did_document, mediator_http_url
   │                                  │                              │
   │                   [phone HTTP POST 到 MCP's mediator]           │
   │<── forward(connection-request) ──│<──── HTTP POST ─────────────│
   │                                  │                              │
   │ [存储 phone_did, phone_mediator_http_url, 注册 peer]            │
   │ [标记为 pending_phone]           │                              │
   │                                  │                              │
   │═══ 三步握手 Step 2: MCP → App ═══                                │
   │                                  │                              │
   │─── forward(connection-response) ─│───── HTTP POST ────────────>│
   │    {accepted, did_document,      │    到 App 的 mediator        │
   │     mediator_http_url,           │                              │
   │     mcp_nonce, mcp_signature}    │                              │
   │                                  │                              │
   │                                  │ [验证 MCP 签名]              │
   │                                  │ [存储 PairedMcp]             │
   │                                  │ [App 侧配对完成 ✓]          │
   │                                  │                              │
   │═══ 三步握手 Step 3: App → MCP ═══                                │
   │                                  │                              │
   │                                  │<──── HTTP POST ─────────────│
   │<── forward(connection-confirm) ──│    到 MCP 的 mediator        │
   │    {phone_nonce, phone_signature}│                              │
   │                                  │                              │
   │ [验证 App 签名]                  │                              │
   │ [pending → paired]               │                              │
   │ [MCP 侧配对完成 ✓]              │                              │
   │                                  │                              │
   │═══ 配对完成 ═══                   │                              │
```

### 1.2 消息路由架构

**点对点直连对方 mediator**：每个参与方直接连接对方的 mediator 发送消息，没有 mediator-to-mediator 的转发。

- **App → MCP**：App 通过 HTTP POST 到 MCP 的 mediator 发送 forward 包装的消息
- **MCP → App**：MCP 通过 HTTP POST 到 App 的 mediator 发送 forward 包装的消息
- **同一 mediator 优化**：若双方使用同一个 mediator，直接通过各自的 WS 连接发送（无需临时连接）

forward 消息格式（DIDComm Routing 2.0 协议）：

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

### 1.3 步骤详解

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
- QR 中直接包含 **HTTP URL**（`https://...`），而非 WSS URL
- App 无需转换即可用于 HTTP POST 发送消息

QR 内容：`didcomm://?_oob=<base64url(JSON)>`

OOB Invitation 消息结构（精简版，不含 DID Document）：

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

关键点：`service_endpoint` 为 HTTP URL，App 直接用于 POST 消息。

#### Step 3: App 扫码并连接 Mediator

App 检测 `didcomm://` 前缀 → 解析 OOB invitation：
1. Base64url 解码 `_oob` 参数
2. 提取 MCP DID、mediator HTTP 地址
3. 连接自己的 mediator（执行 ws-challenge + coordinate-mediation 握手）

#### Step 4: App 发送 connection-request（三步握手 Step 1）

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

关键字段：
- `mediator_http_url`：App 告知 MCP 自己的 mediator HTTP 地址
- `did_document`：App 的 DID Document，使 MCP 可以加密通信

发送方式：App 将 connection-request 包装在 forward 消息中，HTTP POST 到 MCP 的 mediator。

#### Step 5: MCP 发送 connection-response（三步握手 Step 2）

MCP 收到 connection-request 后：
1. 提取 App DID，注册为加密 peer
2. 提取 `mediator_http_url`，保存
3. **存储为 pending 状态**
4. 生成随机 nonce，用 Ed25519 签名
5. 发送 connection-response（含签名）回 App

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

#### Step 6: App 验证 MCP 签名，发送 connection-confirm（三步握手 Step 3）

App 收到 connection-response 后：
1. 使用 `verify_did_signature(mcp_did, mcp_nonce, mcp_signature)` 验证 MCP 签名
2. 若验证成功：存储 PairedMcp（DID、DID Doc、mediator HTTP URL），**App 侧配对完成**
3. 生成自己的随机 nonce，用 Ed25519 签名
4. 发送 connection-confirm 给 MCP

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

#### Step 7: MCP 验证 App 签名，配对完成

MCP 收到 connection-confirm 后：
1. 检查是否有该 DID 的 pending 配对
2. 使用 `verify_did_signature(phone_did, nonce, signature)` 验证 App 的签名
3. 若验证成功：pending → paired，**MCP 侧配对完成**
4. 若验证失败：清除 pending

**安全性说明**：
- DIDComm 消息通过 Ed25519 签名认证，不可伪造
- 双向签名验证：MCP 在 Step 2 签名证明身份，App 在 Step 3 签名证明身份
- App 先完成配对（验证 MCP 签名后），MCP 后完成（验证 App 签名后）
- 配对消息通过 mediator 以明文 JSON 传输（非 JWE 加密），安全性由签名保证

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
| 三步握手双向签名验证 | 有 | 有 |
| 启动时自动打印 QR | 有 | 有 |

### 2.3 客户端 (App) 实现状态

| 组件 | ignite_pay_app | ignite_pay_merchant_app |
|------|---------------|------------------------|
| QR 扫描（mobile_scanner） | 已实现 | 待实现 |
| `didcomm://` OOB 解析 | 已实现 | 待实现 |
| WS challenge-response 认证 | 已实现 | 待实现 |
| coordinate-mediation 握手 | 已实现 | 待实现 |
| connection-request 发送 | 已实现 | 待实现 |
| MCP 签名验证 | 已实现 | 待实现 |
| payment-auth-request 接收 | 已实现 | 待实现 |

---

## 3. DIDComm 消息类型汇总

### 3.1 配对相关

| 消息类型 | 方向 | 说明 |
|---------|------|------|
| `out-of-band/2.0/invitation` | MCP → App (via QR) | OOB 邀请，含 DID、mediator HTTP 地址 |
| `ignite-pay/1.0/connection-request` | App → MCP | 配对请求，含 did_document、mediator_http_url |
| `ignite-pay/1.0/connection-response` | MCP → App | 配对响应，含 did_document、mediator_http_url、mcp_nonce、mcp_signature |
| `ignite-pay/1.0/connection-confirm` | App → MCP | 签名确认，含 phone_nonce + phone_signature |

### 3.2 消息路由

| 消息类型 | 协议 | 说明 |
|---------|------|------|
| `routing/2.0/forward` | 标准 DIDComm | 包装消息，mediator 根据 `body.next` 路由到目标 DID |

### 3.3 Mediator 握手

| 消息类型 | 协议 | 说明 |
|---------|------|------|
| `ignite-pay/1.0/ws-challenge` | 自定义 | WS 认证挑战（含 nonce） |
| `ignite-pay/1.0/ws-challenge-response` | 自定义 | Ed25519 签名 + DID Document |
| `ignite-pay/1.0/ws-auth-ok` | 自定义 | 认证成功确认 |
| `coordinate-mediation/2.0/mediate-request` | 标准 DIDComm | 请求 mediation |
| `coordinate-mediation/2.0/mediate-grant` | 标准 DIDComm | 授予 mediation |
| `coordinate-mediation/2.0/keylist-update` | 标准 DIDComm | 注册路由键 |
| `peer-did-discovery/1.0/discover` | 自定义 | 共享 DID Document |

### 3.4 消息拾取（离线消息）

| 消息类型 | 协议 | 说明 |
|---------|------|------|
| `messagepickup/3.0/status-request` | 标准 DIDComm | 查询排队消息数量 |
| `messagepickup/3.0/batch-pickup` | 标准 DIDComm | 批量拉取离线消息 |

### 3.5 支付流程（配对后使用）

| 消息类型 | 方向 | 说明 |
|---------|------|------|
| `ignite-pay/1.0/payment-auth-request` | MCP → App | 支付授权请求 |
| `ignite-pay/1.0/payment-auth-response` | App → MCP | 支付授权响应（含 session key） |
| `ignite-pay/1.0/create-channel-request` | Merchant App → MCP | 建立状态通道请求 |
| `ignite-pay/1.0/channel-payment-confirm` | MCP → Merchant App | 通道支付确认 |
| `ignite-pay/1.0/session-fund-request` | MCP → Phone | F3/F7: 会话余额不足时请求充值 |
| `ignite-pay/1.0/session-fund-response` | Phone → MCP | F3/F7: 充值完成回复 |
| `ignite-pay/1.0/balance-notification` | MCP → Phone | F13: 余额低于阈值通知 |
| `ignite-pay/1.0/session-renew-request` | MCP → Phone | F14: 请求续期会话密钥 |
| `ignite-pay/1.0/session-renew-response` | Phone → MCP | F14: 续期完成回复 |

---

## 4. 设计特点

- **统一配对协议**：用户端和商户端使用相同的 OOB invitation + connection-request 配对流程
- **非标准 DIDExchange**：使用自定义 `ignite-pay/1.0/connection-request` 而非 Aries DIDExchange 协议
- **HTTP URL 直接内嵌 QR**：OOB invitation 中直接包含 HTTP URL，App 无需转换
- **点对点直连 mediator**：每个参与方直接连接对方的 mediator 发送 forward 包装的消息
- **双向 mediator 地址交换**：App 在 connection-request 中告知 MCP 自己的 mediator HTTP 地址
- **DID Document 内嵌请求**：App 和 MCP 在请求中互相提供 DID Document
- **三步握手双向签名验证**：
  1. MCP 在 connection-response 中发送签名 nonce
  2. App 验证 MCP 签名后存储配对信息，发送自己的签名 nonce
  3. MCP 验证 App 签名后完成配对
- **pending 状态**：MCP 收到 connection-request 后标记为 pending，验证 App 签名后才完成配对
- **明文 JSON 消息处理**：配对消息通过 mediator 以明文传输，安全性由 Ed25519 签名保证
- **持久化配对**：App 侧使用 SharedPreferences 存储 PairedMcp，MCP 侧使用 sled 存储

---

## 5. 关键文件

### 5.1 共享协议库

| 文件 | 职责 |
|------|------|
| `ignite-pay-core/src/didcomm.rs` | DIDComm 消息构建、OOB invitation、pack/unpack |
| `ignite-pay-core/src/identity.rs` | `did:ignite` 生成、DID Document 构建、签名/验证 |

### 5.2 用户端

| 文件 | 职责 |
|------|------|
| `ignite-pay-mcp/src/mediator.rs` | MCP mediator 连接、OOB 邀请生成、入站消息处理、三步握手 |
| `ignite-pay-mcp/src/main.rs` | `generate_pairing_invitation` 工具、启动时自动 QR |
| `ignite_pay_app/lib/qr_scanner_screen.dart` | QR 扫描 UI |
| `ignite_pay_app/lib/services/didcomm_service.dart` | `parseInvitationAndConnect()`、mediator 连接、签名验证 |
| `ignite_pay_app/rust/src/api/ws_client.rs` | Rust WS 客户端：mediator 握手、消息队列 |
| `ignite_pay_app/rust/src/api/simple.rs` | `sign_nonce()`、`verify_did_signature()`、HTTP 认证 |

### 5.3 商户端

| 文件 | 职责 |
|------|------|
| `ignite-pay-merchant-mcp/src/mediator.rs` | 商户 MCP mediator 连接、OOB 邀请生成、三步握手 |
| `ignite-pay-merchant-mcp/src/main.rs` | 启动时自动生成 QR |
| `ignite_pay_merchant_app/rust/src/api/merchant_didcomm.rs` | 商户 App DIDComm 通信 |
