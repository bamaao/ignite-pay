**基于 `did:ignite` 与 DIDComm V2 的 AI Agent 支付身份实现文档**。该文档定义了 Ignite Pay 系统中 `did:ignite` DID 方法的规范，以及基于 DIDComm V2 加密通信的支付授权流程。

---

# 实现文档: Ignite Pay `did:ignite` DID 体系

## 1. 概述
### 1.1 定位
本实现采用本地密钥对生成 `did:ignite` 去中心化身份，通过 DIDComm V2 协议在 MCP Server、Mediator 和手机端之间建立加密通信通道，实现 AI Agent 遇到 HTTP 402 支付挑战时的授权闭环。

### 1.2 核心能力
* **本地身份**：`did:ignite` 基于 Ed25519 密钥对，无需链上注册，本地生成即可使用。
* **加密通信**：通过 DIDComm V2 JWE (authcrypt) 确保端到端消息保密性与真实性。
* **代理路由**：借助 DIDComm Mediator 中继，支持 Agent 与手机端之间的异步授权。
* **即时支付**：基于 X402 协议解析 402 响应，配合金额阈值策略实现自动或交互式支付。
* **可信背书**：通过平台签名机制（VC Attestation）为合法商家签发 Verifiable Credential，MCP/Skill 在处理 402 时验证商家合法性。
* **策略缓存**：基于 IPFS 存储用户黑白名单，MCP/Skill 启动时拉取并存入 sled 本地缓存，实现快速风控决策。

---

## 2. `did:ignite` DID 方法规范

### 2.1 标识符格式

```
did:ignite:z<multibase-base58btc>
```

- **前缀**：`did:ignite:`
- **多碱基指示符**：`z`（表示 base58btc 编码）
- **编码内容**：`0xed 0x01`（multicodec Ed25519 公钥前缀）+ 32 字节 Ed25519 公钥

示例：`did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK`

### 2.2 密钥体系

| 用途 | 算法 | 密钥尺寸 | DID Document 片段 ID |
| :--- | :--- | :--- | :--- |
| 签名/验证 | Ed25519 | 32 字节 | `#key-signing-1` |
| 密钥协商 (加密) | X25519 | 32 字节 | `#key-agreement-1` |

### 2.3 DID Document 结构

```json
{
  "@context": ["https://www.w3.org/ns/did/v1"],
  "id": "did:ignite:z6Mk...",
  "verificationMethod": [{
    "id": "did:ignite:z6Mk...#key-signing-1",
    "type": "Ed25519VerificationKey2020",
    "controller": "did:ignite:z6Mk...",
    "publicKeyMultibase": "z<multibase-base58btc(0xed+0x01+ed25519_pubkey)>"
  }],
  "keyAgreement": [{
    "id": "did:ignite:z6Mk...#key-agreement-1",
    "type": "X25519KeyAgreementKey2020",
    "controller": "did:ignite:z6Mk...",
    "publicKeyBase64": "<base64-nopad(x25519_pubkey)>"
  }]
}
```

### 2.4 身份生命周期

1. **生成**：调用 `generate_ignite_did()` 生成 Ed25519 密钥对，从公钥推导 DID 标识符。
2. **注册**：通过 DIDComm Agent 注册密钥，用于后续签名与加密。
3. **发布**：在 Mediator 握手阶段通过 `peer-introduction` 发送完整 DID Document。
4. **解析**：接收方通过 `parse_did_document()` 从 DID Document 提取公钥并注册为通信对等方。

---

## 3. 系统架构

### 3.1 组件与通信拓扑

```
AI Agent (Claude/etc)
  │  stdio (JSON-RPC 2.0 / MCP)
  ▼
ignite-pay-mcp (MCP Server)
  │  WebSocket (DIDComm JWE)
  ▼
didcomm-mediator
  │  DIDComm forward / pickup
  ▼
Phone (Flutter App)
```

### 3.2 存储架构

| 层级 | 技术实现 | 存储内容 | 持久化 |
| :--- | :--- | :--- | :--- |
| **身份层** | 内存 (DIDCommAgent) | `did:ignite` 密钥对、对等方公钥 | 进程生命周期 |
| **支付层** | sled (嵌入式 KV) | PaymentRequest 记录、状态、交易签名 | 持久化至磁盘 |
| **授权层** | DashMap (内存) | PendingAuthStore（oneshot channel 映射） | 进程生命周期 |
| **策略层** | IPFS (去中心化存储) + sled (本地缓存) | 黑白名单、商家 VC | IPFS 持久化 + sled 本地缓存 |
| **信任层** | 平台 DID (内置) | 平台签名公钥、VC 验证逻辑 | 随版本发布 |

### 3.3 配置

通过 `config.toml`（或 `IGNITE_PAY_CONFIG` 环境变量指定路径）加载：

```toml
[mediator]
ws_url = "ws://127.0.0.1:8080/ws"
phone_did = ""

[storage]
path = "./data"

[policy]
auto_approve_max = 0      # 0 = 禁用自动批准
auth_timeout = 300         # 授权超时（秒）
```

---

## 4. 核心流程规范

### 4.1 Mediator 连接握手

MCP Server 启动时通过 WebSocket 连接到 Mediator，执行三步明文握手：

| 步骤 | 方向 | 消息类型 | 说明 |
| :--- | :--- | :--- | :--- |
| 1 | Client -> Mediator | `coordinate-mediation/2.0/mediate-request` | 注册为 Mediator 客户端 |
| 2 | Mediator -> Client | `coordinate-mediation/2.0/mediate-grant` | Mediator 确认 |
| 3 | Client -> Mediator | `coordinate-mediation/2.0/keylist-update` (add) | 注册接收密钥 `{did}#key-1` |
| 4 | Mediator -> Client | `coordinate-mediation/2.0/keylist-update-response` | 确认密钥注册 |
| 5 | Client -> Mediator | `peer-did-discovery/1.0/discover` | 发送完整 DID Document |

握手完成后进入加密消息接收循环。断连后每 3 秒自动重连。

### 4.2 X402 支付挑战处理

当 AI Agent 遇到 HTTP 402 响应时，调用 MCP Tool `process_x402_challenge`：

**输入**：
```json
{
  "challenge_body": "<402 响应 JSON>",
  "phone_did": "did:ignite:z..."
}
```

**402 响应解析**：从 `accepts` 数组第一个元素提取支付参数：

| 字段 | 用途 | 缺省值 |
| :--- | :--- | :--- |
| `paymentType` | 支付类型 | `"transfer"` |
| `network` | 网络 | `"unknown"` |
| `token` | 代币标识 | `"unknown"` |
| `amount` | 金额（最小单位） | `0` |
| `recipient` | 收款方 | `"unknown"` |

**决策流程**：

```
收到 402
  │
  ├─ 黑名单命中 (merchant_did 在黑名单中) ?
  │    YES → 直接阻断，返回拒绝
  │
  │    NO ↓
  │
  ├─ 白名单命中 + 额度内 (merchant_did 在白名单中 && amount <= list_max_amount) ?
  │    YES → 自动批准，执行 Mock 支付，返回 tx 签名
  │
  │    NO ↓
  │
  ├─ amount <= auto_approve_max && auto_approve_max > 0 ?
  │    YES → 执行 Mock 支付，返回 tx 签名
  │
  │    NO ↓
  │
  ├─ 创建 PaymentRequest (status: PendingAuth)
  ├─ 保存至 sled
  ├─ 构建 DIDComm 授权请求消息
  ├─ JWE 加密 (authcrypt) → 通过 Mediator 发送至手机端
  └─ 等待手机响应 (timeout: auth_timeout 秒)
       │
       ├─ true  → 执行 Mock 支付，状态 → Executed
       ├─ false → 状态 → Rejected
       └─ 超时  → 状态 → Expired
```

### 4.3 授权消息

#### 4.3.1 授权请求（MCP Server → 手机端）

MCP Server 发送到手机端的 DIDComm 授权消息：

* **消息类型**：`https://didcomm.org/ignite-pay/1.0/payment-auth-request`
* **加密方式**：JWE authcrypt（通过 `pack_authcrypt`）
* **消息体**：

| 字段 | 类型 | 说明 |
| :--- | :--- | :--- |
| `payment_id` | string | UUID |
| `merchant_did` | string | 收款方 DID |
| `amount` | number | 金额 |
| `description` | string | 人类可读描述 |

#### 4.3.2 授权响应（手机端 → MCP Server）

手机端返回给 MCP Server 的 DIDComm 授权响应消息：

* **消息类型**：`https://didcomm.org/ignite-pay/1.0/payment-auth-response`
* **加密方式**：JWE authcrypt
* **消息体**：

| 字段 | 类型 | 必填 | 说明 |
| :--- | :--- | :--- | :--- |
| `payment_id` | string | 是 | 对应的支付请求 UUID |
| `authorized` | bool | 是 | 是否授权此次支付 |
| `list_action` | string | 是 | 名单操作：`"add_whitelist"` / `"add_blacklist"` / `"none"` |
| `list_label` | string | 否 | 用户自定义备注（如 `"ShopX Marketplace"`），`list_action` 非 `"none"` 时建议填写 |
| `list_max_amount` | number | 否 | 对该商家的自动批准上限（最小单位），仅 `"add_whitelist"` 时有效 |

### 4.4 Mock 支付执行

当前阶段使用 Mock 支付，交易签名格式为：

```
tx_mock_{payment_id}_{uuid_v4}
```

### 4.5 平台签名与商家准入

为确保 AI Agent 仅向合法商家支付，引入平台签名背书机制。平台作为可信第三方，为经过审核的商家签发 Verifiable Credential (VC)，MCP/Skill 在处理 402 时验证商家合法性。

**完整流程**：

```
商家申请                  平台背书                     支付验证
────────                 ────────                    ────────
提交 DID + Metadata  →   平台审核                   MCP/Skill 收到 402
                         │                           │
                         ├─ 审核通过 → 签发 VC       ├─ 提取 merchant_did
                         │   (平台私钥签名)           ├─ 查找对应 VC
                         │                           ├─ 用内置平台公钥验证签名
                         └─ 审核拒绝                  ├─ 检查有效期
                                                     │
                                                     ├─ 验证通过 → 进入决策流程
                                                     └─ 验证失败 → 拒绝支付
```

**详细步骤**：

1. **商家申请**：商家向平台提交 `did:ignite` 标识符及服务元数据（名称、类型、描述等）。
2. **平台背书**：平台审核通过后，使用平台私钥签发 VC，包含商家 DID、有效期、服务类型等声明。
3. **X402 携带**：服务商在 402 响应中附带 VC（直接嵌入或通过 IPFS CID 引用）。
4. **MCP/Skill 校验**：收到 402 后，使用内置的平台公钥验证 VC 签名的真实性和有效期，确认商家合法性。

**VC 结构定义**：

```json
{
  "@context": ["https://www.w3.org/2018/credentials/v1"],
  "type": ["VerifiableCredential", "IgniteMerchantCredential"],
  "issuer": "did:ignite:z6Mk...<platform_did>",
  "issuanceDate": "2025-01-01T00:00:00Z",
  "credentialSubject": {
    "id": "did:ignite:z6Mk...<merchant_did>",
    "service_type": "api-service",
    "merchant_name": "Example API Service"
  },
  "expirationDate": "2026-01-01T00:00:00Z",
  "proof": {
    "type": "Ed25519Signature2020",
    "verificationMethod": "did:ignite:z6Mk...<platform_did>#key-signing-1",
    "proofPurpose": "assertionMethod",
    "proofValue": "<ed25519_signature_bytes_base58>"
  }
}
```

| VC 字段 | 类型 | 说明 |
| :--- | :--- | :--- |
| `issuer` | string | 平台 DID，标识签发方 |
| `credentialSubject.id` | string | 商家 DID，标识被背书方 |
| `credentialSubject.service_type` | string | 商家服务类型（如 `"api-service"`、`"data-provider"`） |
| `credentialSubject.merchant_name` | string | 商家人类可读名称 |
| `expirationDate` | string (ISO 8601) | VC 过期时间 |
| `proof.proofValue` | string | 平台私钥对 VC 内容的 Ed25519 签名 |

### 4.6 IPFS 黑白名单管理

用户可在手机端授权时选择将商家加入黑名单或白名单。名单存储在 IPFS（去中心化、不可篡改），DID Document 中记录当前名单 CID。MCP/Skill 启动时从 IPFS 拉取名单，存入 sled 本地缓存，用于后续快速风控决策。

**完整流程**：

```
手机端授权                     名单同步                        本地决策
────────                     ────────                       ────────
用户选择名单操作           →  MCP/Skill 收到授权响应       →  收到 402
(list_action)                 │                              │
                              ├─ 解析 list_action             ├─ 查 sled 缓存
                              │                              │
                              ├─ "add_whitelist"             ├─ 黑名单命中 → 阻断
                              │   追加到白名单缓存            │
                              │                              ├─ 白名单命中 + 额度内 → 批准
                              ├─ "add_blacklist"             │
                              │   追加到黑名单缓存            └─ 其余 → 走手机授权
                              │
                              ├─ 异步上传至 IPFS
                              │   (更新 CID)
                              │
                              └─ DIDComm V2 通知手机端新 CID
```

**详细步骤**：

1. **名单存储**：黑白名单以 JSON 文件形式存储在 IPFS，用户的 DID Document 中通过 `service` 端点记录当前名单 CID。
2. **启动拉取**：MCP/Skill 启动时，从 DID Document 中读取名单 CID，从 IPFS 拉取名单数据，解析后存入 sled 本地缓存。
3. **本地决策**：收到 402 时优先查询 sled 本地缓存：
   - 黑名单命中 → 直接阻断，返回拒绝
   - 白名单命中 + 额度内 → 自动批准，执行支付
   - 其余情况 → 走手机授权流程

**名单结构定义**：

```json
{
  "version": 1,
  "owner_did": "did:ignite:z6Mk...<user_did>",
  "updated_at": "2025-06-15T10:30:00Z",
  "whitelist": [
    {
      "did": "did:ignite:z6Mk...<merchant_did>",
      "label": "ShopX Marketplace",
      "max_amount": 1000000,
      "expires": "2026-06-15T00:00:00Z"
    }
  ],
  "blacklist": [
    {
      "did": "did:ignite:z6Mk...<merchant_did>",
      "label": "Suspicious API",
      "expires": null
    }
  ]
}
```

| 名单条目字段 | 类型 | 必填 | 说明 |
| :--- | :--- | :--- | :--- |
| `did` | string | 是 | 商家 `did:ignite` 标识符 |
| `label` | string | 是 | 用户自定义备注（如 `"ShopX Marketplace"`） |
| `max_amount` | number | 否 | 对该商家的自动批准上限（最小单位），仅白名单条目使用 |
| `expires` | string (ISO 8601) / null | 否 | 名单条目过期时间，`null` 表示永不过期 |

### 4.7 名单同步流程

当手机端返回授权响应（`payment-auth-response`）后，MCP/Skill 需根据响应中的 `list_action` 字段同步更新黑白名单：

**完整流程**：

```
手机端返回 payment-auth-response
  │
  ├─ 解析 authorized 字段 → 处理支付授权结果
  │
  ├─ 解析 list_action 字段
  │    │
  │    ├─ "add_whitelist"
  │    │    ├─ 构建白名单条目: { did, label, max_amount, expires }
  │    │    ├─ 追加到 sled 本地缓存 (whitelist)
  │    │    ├─ 异步上传: 合并名单 → JSON → IPFS → 获取新 CID
  │    │    └─ DIDComm V2 通知手机端新 CID
  │    │
  │    ├─ "add_blacklist"
  │    │    ├─ 构建黑名单条目: { did, label, expires }
  │    │    ├─ 追加到 sled 本地缓存 (blacklist)
  │    │    ├─ 异步上传: 合并名单 → JSON → IPFS → 获取新 CID
  │    │    └─ DIDComm V2 通知手机端新 CID
  │    │
  │    └─ "none"
  │         └─ 无名单操作，仅处理支付授权结果
  │
  └─ 流程结束
```

**通知消息**（MCP/Skill → 手机端）：

* **消息类型**：`https://didcomm.org/ignite-pay/1.0/list-sync-notification`
* **消息体**：

| 字段 | 类型 | 说明 |
| :--- | :--- | :--- |
| `list_cid` | string | IPFS 上新名单的 CID |
| `action` | string | 执行的操作：`"add_whitelist"` / `"add_blacklist"` |
| `target_did` | string | 被操作的商家 DID |
| `timestamp` | string | 同步时间戳 (ISO 8601) |

---

## 5. MCP 工具接口

| 工具名 | 输入 | 输出 |
| :--- | :--- | :--- |
| `process_x402_challenge` | `challenge_body`, `phone_did` | 支付结果 + tx 签名 / 错误信息 |
| `check_authorization` | `payment_id` | 支付状态、金额、时间、tx 签名 |
| `get_payment_history` | `limit` (默认 10) | 最近 N 条支付记录 |
| `get_identity` | (无) | 当前 `did:ignite`、Mediator 连接状态 |

---

## 6. Mediator 支持的协议

| 协议 | 版本 | 消息类型 |
| :--- | :--- | :--- |
| Coordinate Mediation | 2.0 | `mediate-request`, `mediate-grant`, `keylist-update`, `keylist-update-response` |
| Routing | 2.0 | `forward` |
| Message Pickup | 3.0 | `status-request`, `status`, `batch-pickup`, `batch`, `live-delivery-request` |
| Peer DID Discovery | 1.0 | `discover` |

---

## 7. 安全设计

* **端到端加密**：授权请求通过 DIDComm JWE authcrypt 加密，Mediator 无法读取消息内容，仅做路由转发。
* **密钥隔离**：`did:ignite` 密钥对由 DIDCommAgent 管理，MCP Server 通过 `Arc<Mutex<DIDCommAgent>>` 受控访问。
* **超时保护**：未获授权的支付请求在 `auth_timeout` 秒后自动过期，防止无限挂起。
* **重连机制**：Mediator 断连后每 3 秒自动重连并重新执行完整握手。

---

## 8. 当前状态与演进方向

| 阶段 | 内容 | 状态 |
| :--- | :--- | :--- |
| **V0.1** (当前) | `did:ignite` 本地身份 + DIDComm V2 通信 + Mock 支付 + MCP Server | ✅ 已实现 |
| **V1.0** | 手机端接收 DIDComm 授权消息、返回授权结果；端到端授权链路打通 | 待开发 |
| **V1.1** | 平台签名/VC 商家背书 + IPFS 黑白名单 + 手机端名单管理 + sled 本地缓存风控决策 | 待开发 |
| **V2.0** | 接入 Solana 链上支付、ZK Compression 存储 DID | 待开发 |

---

> **实现备注**：当前 V0.1 阶段专注于验证 `did:ignite` 身份模型与 DIDComm V2 加密通信的可行性。支付执行为 Mock 实现，手机端授权回调尚未接入。核心身份模块 (`identity.rs`) 与 DIDComm 模块 (`didcomm.rs`) 已在 `ignite-pay-skill` 和 `ignite-pay-mcp` 之间复用，为后续阶段提供稳定基础。
