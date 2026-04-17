**基于 DIDComm-V2 OOB (Out-of-Band) 协议的极简互信连接方案**

该方案完全脱离了对 Solana 链上 Root DID 的强制依赖，转而通过 **Peer-to-Peer (P2P)** 模式实现手机端与 MCP Server 的安全握手与指令分发。

---

# 用户端：基于 DIDComm-V2 OOB 的极简连接方案

## 1. 核心设计思想
* **零链上依赖**：不需要 Root DID 上链，节省 Gas 费，降低用户门槛。
* **首次握手即信任 (TOFU)**：通过扫码（带外通信）建立初始信任。
* **双向对等身份**：手机和 Server 各自持有独立的 `did:ignite` 标识。
* **Relay 中转**：利用 Mediator 服务实现公网异步通信（WebSocket + HTTP）。

---

## 2. 角色与身份定义

| 实体 | 身份类型 | 职责 |
| :--- | :--- | :--- |
| **Mobile App** | `did:ignite:z<multibase>` | 发起控制请求，管理已连接的 Server 列表。 |
| **MCP Server** | `did:ignite:z<multibase>` | 监听 Mediator 消息，执行 MCP Tool 并回传结果。 |
| **Mediator (didcomm-router)** | Mediator / Relay | 消息转发路由、离线队列、FCM 推送通知。不参与解密。 |

### DID 标识格式

系统使用自定义 `did:ignite` 方法（非 `did:peer`），格式为：

```
did:ignite:z<multibase-encoded-public-key>
```

编码规则：
1. Ed25519 公钥 (32 bytes) → 添加 multicodec 前缀 `[0xed, 0x01]` → 共 34 bytes
2. Base58 编码 → 拼接前缀 `did:ignite:z`

> 示例：`did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK`

### DID Document 结构

每个 `did:ignite` 标识对应一个 W3C DID Document，包含 Ed25519 签名密钥和 X25519 密钥协商密钥：

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

- **Ed25519**：用于身份签名（`verificationMethod`），证明"我是谁"
- **X25519**：用于消息加密（`keyAgreement`），负责 ECDH 密钥协商

---

## 3. 连接建立流程 (OOB Handshake)

### 3.1 Server 端生成邀请 (Out-of-Band Invitation)
MCP Server 启动后，若未绑定设备，则通过 CLI 或日志生成一个 **OOB 邀请包**。

**生成过程**（`ignite-pay-mcp/src/mediator.rs`）：
1. 调用 `build_oob_invitation(our_did, label, mediator_ws_url, did_doc)` 构造 OOB 消息
2. 将消息 JSON 序列化，base64url 编码
3. 格式化为 `didcomm://?_oob=<base64url>` URL
4. 可选：调用 `generate_invitation_qr()` 生成 ASCII 二维码

**OOB 邀请消息格式**：
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

**扫码 URL 格式**：`didcomm://?_oob=<base64url-encoded-invitation-json>`

### 3.2 手机端扫码响应
1. **扫码**：手机 App 解析 OOB 邀请 URL，base64url 解码，提取 Server 的 `did:ignite`、DID Document 和 Mediator WebSocket 地址。
2. **生成本地 DID**：App 调用 `generate_ignite_did()` 为此 Server 生成一个独立的 `did:ignite` 标识（Ed25519 + X25519 密钥对）。
3. **连接 Mediator**：通过 WebSocket 连接到 Mediator，完成 WS 认证握手（挑战-响应）。
4. **发送 connection-request**：通过 Mediator 向 Server 发送 `https://didcomm.org/ignite-pay/1.0/connection-request` 消息（包含 push channel 和可选 FCM token）。

### 3.3 建立互信 (White-listing)
* **Server 端验证**：Server 收到 `connection-request` 后，将手机端的 `did:ignite` 通过 `add_peer_from_doc()` 注册到本地 DIDComm Agent，记录在白名单中。
* **Server 端响应**：返回 `connection-response`（`accepted: true/false`）。
* **后续通信**：双方使用 Authcrypt（JWE 认证加密）通信。每次加密时，发送方使用自己的 X25519 私钥和接收方的 X25519 公钥执行 ECDH 密钥协商，派生对称加密密钥（AES-256-GCM 或 XChaCha20-Poly1305），无需独立的协商步骤。

---

## 4. 指令路由与安全协议 (DIDComm V2)

### 4.1 消息封装模型
通过 Mediator 安全传输的消息封装层次：

1. **Inner Layer (MCP Payload)**：原始业务数据（如支付授权请求、授权响应）。
2. **Encryption Layer (Authcrypt JWE)**：发送方使用 `affinidi-messaging-didcomm` 库的 `pack_authcrypt()` 方法，基于双方 X25519 密钥执行 ECDH + AES-GCM 加密，生成 JWE。
3. **Forward Layer (Routing)**：加密后的 JWE 作为 `https://didcomm.org/routing/2.0/forward` 消息体，发送到 Mediator，由 Mediator 路由到目标接收方。

### 4.2 MCP 指令交互示例
* **手机 → Server**：
  `App -> pack_authcrypt(MCP_Call) -> Forward -> Mediator -> Server -> unpack()`
* **Server → 手机**：
  `Server -> pack_authcrypt(MCP_Result) -> Forward -> Mediator -> App -> unpack()`

### 4.3 Mediator 握手协议
MCP Server 连接 Mediator 时执行以下握手流程（全部明文）：

| 步骤 | 消息类型 | 说明 |
|:---|:---|:---|
| 1 | `mediate-request` | 请求 Mediator 为本 DID 提供中转服务 |
| 2 | `mediate-grant` | Mediator 授权 |
| 3 | `keylist-update` | 注册 DID 路由信息 |
| 4 | `keylist-update-response` | 确认注册 |
| 5 | `peer-introduction` | 发送 DID Document 供 Mediator 转发 |
| 6 | `status-request` | 查询离线消息数量 |
| 7 | `batch-pickup` | 拉取离线消息 |

握手完成后进入双向消息循环。

---

## 5. 开发实施要点 (Implementation)

### 5.1 手机端 (App)
* **持久化**：需使用安全存储（KeyChain/EncryptedSharedPreferences）保存每个 Server 的 `did:ignite` 私钥（Ed25519 signing key + X25519 key agreement key）。
* **离线消息拉取**：通过 Mediator 的 REST API 拉取离线消息：
  - `GET /v1/sync/list` — 分页查询排队消息
  - `GET /v1/sync/messages/{msg_id}` — 获取单条消息
* **Message Pickup 3.0**：App 连接 Mediator 后，通过 `status-request` 和 `batch-pickup` 拉取离线期间积压的消息。

### 5.2 MCP Server 端 (Rust)
* **自动化邀请**：Server 初始化时检测已配对设备，若为空则自动生成 OOB 二维码。
* **权限拦截器**：
  ```rust
  // 实际代码：MCP 处理前检查白名单
  if !agent.has_peer(sender_did) {
      return Err("Unauthorized_DID");
  }
  ```
* **风险控制引擎**：通过 `ListStore`（`ignite-pay-core/src/list_store.rs`）实现白名单/黑名单管理：
  - `risk_check(merchant_did, amount)` → `Blocked` / `AutoApproved` / `NeedsAuth`
  - 支持条目过期时间和金额上限
  - 支持通过 IPFS 跨设备同步列表

### 5.3 撤销与权限管理
权限管理通过 `ListStore` 程序化实现，不限于物理端操作：
* **白名单移除**：`remove_from_whitelist(did)` 即可撤销对某 DID 的自动授权
* **黑名单添加**：`add_to_blacklist(did, expires)` 可阻止特定 DID
* **风险降级**：从白名单移除后，该 DID 的后续请求会降级为 `NeedsAuth`（需手动确认）
* Mediator 也支持管理端通过 `reset-peers` 重置所有配对关系

---

## 6. Mediator (didcomm-router) 服务

### 6.1 传输方式
* **WebSocket** (`GET /ws`)：双向实时通信，支持在线消息推送
* **HTTP** (`POST /`)：单向消息投递

### 6.2 认证机制
* **WebSocket 认证**：挑战-响应模式。Mediator 发送包含 nonce 的明文挑战，客户端返回 JWE 加密的挑战响应，证明密钥所有权。
* **REST API 认证**：JWT Bearer Token。通过 `GET /v1/auth/challenge` 获取 nonce，`POST /v1/auth/token` 用 DID 签名交换 JWT。

### 6.3 推送通知
* 支持 **FCM (Firebase Cloud Messaging)** 推送。当接收方不在线时，Mediator 将消息入队并通过 FCM 通知手机端。
* 客户端通过 `POST /v1/devices/register-token` 注册 FCM token。

### 6.4 离线消息
* Mediator 为每个注册的 DID 维护消息队列（sled 持久化存储）
* 支持在线方实时推送和离线方拉取两种模式
* 消息去重：基于消息 ID 的 DashMap 去重，防止重放攻击

### 6.5 完整端点列表

| 方法 | 路由 | 功能 |
|:---|:---|:---|
| POST | `/` | 接收 DIDComm 消息 (HTTP) |
| GET | `/ws` | WebSocket 连接 |
| GET | `/health` | 健康检查 |
| GET | `/v1/auth/challenge` | 获取认证 nonce |
| POST | `/v1/auth/token` | DID 签名换 JWT |
| GET | `/v1/sync/list` | 分页查询排队消息 |
| GET | `/v1/sync/messages/{msg_id}` | 获取单条消息 |
| POST | `/v1/devices/register-token` | 注册 FCM 推送 token |
| POST | `/v1/agents/bind` | Agent 绑定用户 |
| POST | `/v1/agents/{agent_id}/command` | 命令转发 |

---

## 7. 总结
本方案利用 **DIDComm V2** 的 Authcrypt 加密能力，基于自定义 `did:ignite` 标识方法，在**不需要区块链参与**的情况下，为 MCP Server 提供了端到端加密的通信安全。它优化了用户体验，使商户或开发者能够以"扫码即连"的方式快速部署自己的 AI Agent 控制系统。

---

**给开发的建议：**
系统使用 `affinidi-messaging-didcomm` (v0.13) 作为 DIDComm 引擎。DID 标识通过 `ignite-pay-core/src/identity.rs` 的 `generate_ignite_did()` 生成，DID Document 通过 `build_did_document()` 构造。所有 DIDComm 消息加密/解密通过 `ignite-pay-core/src/didcomm.rs` 的 `pack_encrypted()` 和 `unpack_message()` 完成。

### 关键依赖库
* **Rust**: `affinidi-messaging-didcomm` — DIDComm V2 消息加密/解密
* **密钥类型**: Ed25519（签名）+ X25519（密钥协商），成对出现
* **加密模式**: Authcrypt only（JWE 认证加密），未实现 Anoncrypt

### 给开发同学的 Check-List
1. **密钥管理**：每个连接需要独立的 `did:ignite` 密钥对（Ed25519 + X25519），私钥存储在安全存储中
2. **DID 解析**：`did:ignite` 为本地解析格式，不需要外部解析器。密钥数据编码在 DID Document 中，通过 `parse_did_document()` 提取
3. **加密/解密**：发送方用 `pack_authcrypt(sender, recipient)` 加密，接收方用 `unpack(jwe)` 解密。ECDH 密钥协商在加密过程中自动完成，无需独立协商步骤
