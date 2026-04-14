**基于 DIDComm V2 的 AI Agent 全链路通信方案**。

它整合了 **FCM 信号唤醒**、**HTTPS 主动拉取**以及 **WebSocket 实时通信**，支持两条推送通道：

* **海外用户**：FCM 信号唤醒 + HTTPS 主动拉取（省电，无需维持长连接）。
* **中国用户**：WebSocket 在线直推 + 离线暂存 / 重连后 Pickup 拉取（中国大陆无法使用 FCM）。

这是一个专为高安全、跨国环境（Google Play & iOS）设计的生产级架构。

---

# AI Agent 全链路通信系统技术规约 (DIDComm V2 标准)

## 1. 架构逻辑：门铃与包裹模式 (Signal & Pull)

为了规避 FCM 载荷限制并确保 **DIDComm V2** 的端到端加密安全，系统采用"门铃与包裹"逻辑：

* **FCM (门铃)**：仅发送一个轻量级的通知信号，告诉手机"你有新消息"。
* **HTTPS (包裹)**：手机端通过安全的 HTTPS 通道，主动从服务端拉取完整的加密 DIDComm Message (JWE) 包。
* **DIDComm (拆包)**：加解密与验签完全在手机和 MCP/Skill 本地完成，服务端无法解密消息体。

> **传输路径不对称说明**：
> - **下行链路**（手机→MCP/Skill）：手机使用 HTTPS 提交指令（省电，无需维持长连接），服务端通过 WebSocket 实时推送给 MCP/Skill（MCP/Skill 常在线）。
> - **上行链路——海外用户**（MCP/Skill→手机）：MCP/Skill 通过 WebSocket 上报结果，服务端通过 FCM 信号通知手机，手机再通过 HTTPS 拉取（避免手机维持长连接消耗电量）。
> - **上行链路——中国用户**（MCP/Skill→手机）：MCP/Skill 通过 WebSocket 上报结果，服务端检测用户 `push_channel=websocket`，若 WS 在线则直推 JWE；若离线则暂存，手机重连后通过 Message Pickup 3.0 拉取。

---

## 2. 系统拓扑图

```
┌─────────────────────────────────────────────────────────────────────────┐
│                                                                         │
│    手机 (Flutter App)                 服务端 (Mediator)      MCP/Skill   │
│                                                                         │
│    ┌──────────────┐              ┌──────────────┐           ┌────────┐ │
│    │ DIDComm      │   HTTPS      │ Mediator     │ WebSocket │MCP/    │ │
│    │ Pack/Unpack  │──────────────│              │───────────│Skill   │ │
│    │              │<─────────────│ - 消息暂存    │<──────────│DIDComm │ │
│    │ FCM Listener │              │ - FCM 推送    │           │        │ │
│    └──────────────┘              │ - WS 路由     │           └────────┘ │
│         ↑                        │               │                      │
│         │ FCM Signal             │ - WS 直推*    │                      │
│    ┌──────────────┐              └──────────────┘                      │
│    │ FCM / APNs   │<─────────────── FCM 推送 (海外用户) ────────────    │
│    └──────────────┘                                                    │
│         ↑                                                              │
│         │ WS 直推 (中国用户)*                                          │
│    ┌──────────────┐                                                    │
│    │ WS Listener  │<──── WS 在线直推 JWE / 离线暂存 Pickup ────────    │
│    └──────────────┘                                                    │
│                                                                         │
│    * 中国用户: push_channel=websocket，无 FCM                           │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘

消息流向:

  下行 (手机→MCP/Skill):                  上行 (MCP/Skill→手机) —— 海外:
    Flutter → HTTPS POST → Mediator       MCP/Skill → WebSocket Send → Mediator
    → WebSocket Forward → MCP/Skill       → Redis 暂存 → FCM Signal
    → DIDComm Unpack → 执行               → HTTPS GET → Flutter
                                          → DIDComm Unpack → UI 更新

                                         上行 (MCP/Skill→手机) —— 中国:
                                          MCP/Skill → WebSocket Send → Mediator
                                          → 检查 push_channel=websocket
                                          → WS 在线: 直推 JWE → Flutter
                                          → WS 离线: 暂存 → 重连 Pickup 拉取
                                          → DIDComm Unpack → UI 更新
```

---

## 3. 身份与密钥管理

### 3.1 身份模型

系统使用 **DID** 作为身份标识，每个参与者拥有唯一的 DID 和关联的密钥对：

| 角色 | 身份标识 | 密钥用途 |
|:-----|:---------|:---------|
| 手机 (User) | `did:ignite:user_{uuid}` | 签名指令、解密来自 MCP/Skill 的消息 |
| MCP/Skill | `did:ignite:agent_{uuid}` | 签名反馈、解密来自手机的指令 |
| 服务端 (Mediator) | `did:ignite:mediator` | 不参与 DIDComm 加解密，仅路由转发 |

### 3.2 密钥交换与信任建立

首次使用前，需完成以下绑定流程：

```
手机 (Flutter)                        服务端 (Mediator)              MCP/Skill
    │                                       │                              │
    │  1. 用户注册/登录                       │                              │
    │  (生成 DID + Ed25519 密钥对,            │                              │
    │   私钥存入 flutter_secure_storage)       │                              │
    │ ──────────────────────────────────────>│                              │
    │  { did: "did:ignite:user_xxx",          │                              │
    │    public_key: <Ed25519 pubkey> }        │                              │
    │                                       │                              │
    │  2. 绑定 MCP/Skill                      │                              │
    │ ──────────────────────────────────────>│                              │
    │  { agent_id: "did:ignite:agent_yyy" }  │                              │
    │                                       │                              │
    │                                       │  3. 服务端验证绑定关系         │
    │                                       │     (用户是否有权控制此 MCP/Skill) │
    │                                       │                              │
    │  4. 返回 MCP/Skill 的 DID 文档           │                              │
    │ <──────────────────────────────────────│                              │
    │  { did: "did:ignite:agent_yyy",         │                              │
    │    public_key: <MCP/Skill Ed25519 pubkey>, │                           │
    │    ws_endpoint: "wss://..." }            │                              │
    │                                       │                              │
    │  5. 手机本地缓存 MCP/Skill 公钥          │                              │
    │     (后续 DIDComm 加密使用此公钥)        │                              │
```

> **密钥轮换**：当任一方轮换密钥时，通过 DID 文档更新通知对端。服务端维护最新的 DID 文档缓存。MCP/Skill 在启动时通过 WebSocket 执行 DIDComm Mediator 握手，注册最新密钥。

---

## 4. 全链路交互详述

### 4.1 下行链路：手机控制 MCP/Skill

1.  **消息封装 (Flutter)**：使用本地存储的私钥对指令签名，并针对 MCP/Skill 的公钥进行加密，生成 DIDComm Encrypted Message (JWE)。
2.  **指令提交 (HTTPS)**：通过 `POST /v1/agents/{agent_id}/command` 将 JWE 提交至服务端。`agent_id` 在 URL 路径中指定。
3.  **鉴权与路由 (Server)**：服务端验证 Bearer Token 对应的用户是否有权向该 `agent_id` 发送指令。验证通过后，通过 WebSocket 将 JWE 转发至 MCP/Skill。
4.  **执行 (MCP/Skill)**：MCP/Skill 终端接收消息，本地解密验签后执行。

### 4.2 上行链路：MCP/Skill 反馈至手机 (核心可靠方案)

1.  **结果封装 (MCP/Skill)**：MCP/Skill 遇到 X402 支付挑战时，构建 `payment-auth-request` 加密 JWE（使用用户公钥加密），包含 payment_id、merchant_did、amount、description 等信息。
2.  **数据上报 (WebSocket)**：MCP/Skill 通过 WebSocket 将 JWE 发送至 Mediator。
3.  **暂存与信号 (Server)**：
    * 服务端接收并存入缓存（Redis/DB），生成 `msg_id`。
    * 服务端根据 `agent_id` 查找绑定的用户，验证消息归属。
    * **海外用户**（`push_channel: "fcm"`）：调用 **FCM** 发送 Data Message：`{"type": "SIGNAL", "msg_id": "uuid-123"}`。
    * **中国用户**（`push_channel: "websocket"`）：
        - WS 在线：直接通过 WebSocket 将 JWE 推送至手机端。
        - WS 离线：暂存消息，手机重连后通过 Message Pickup 3.0 协议 (`status-request` / `batch-pickup`) 拉取。
4.  **主动拉取 (Flutter)** —— 仅海外 FCM 通道：
    * Flutter 收到 FCM 信号，发起请求：`GET /v1/sync/messages/uuid-123`。
5.  **解密展示 (Flutter)**：获取 JWE 后，在本地 Isolate 中进行 DIDComm `Unpack`，校验成功后展示支付授权界面（金额、商家、描述）。
6.  **用户授权 (Flutter)**：用户点击"授权"后，Flutter App 创建 Session Key：
    * 生成 Ed25519 临时密钥对。
    * 提交链上交易注册 Session Key（绑定 owner、spending_limit、scopes、expires_at）。
    * 链上确认后，构建 `payment-auth-response` 加密 JWE（包含 `session_key_pubkey`、`session_key_tx_signature` 等字段）。
7.  **授权返回 (Flutter → MCP/Skill)**：Flutter 通过 HTTPS 提交加密响应至 Mediator → Mediator 通过 WebSocket 转发至 MCP/Skill。
8.  **链上支付 (MCP/Skill)**：MCP/Skill 使用收到的 Session Key 代表用户执行链上支付（签名 ExecutePayment 交易），无需再次请求用户授权。

---

## 5. 关键接口与协议定义

### 5.1 双层认证说明

系统使用两层独立的认证机制，各司其职：

| 层级 | 机制 | 目的 | 验证方 |
|:-----|:-----|:-----|:-------|
| **传输层** | HTTPS Bearer Token (JWT) | 验证"谁在调用 API"——确保只有合法用户能拉取/提交消息 | 服务端 (Mediator) |
| **消息层** | DIDComm 签名 (`Unpack` 的 `authenticated`) | 验证"消息是谁发的"——确认消息确实由绑定的 MCP/Skill/用户发出 | 客户端 (Flutter/MCP/Skill) |

Bearer Token 是用户登录后服务端签发的 JWT，包含 `user_did` 字段。Token 与 DID 身份绑定：服务端通过 Token 中的 `user_did` 查找其绑定的 MCP/Skill 列表，实现鉴权。

### 5.2 指令提交接口 (下行)

* **Endpoint**: `POST /v1/agents/{agent_id}/command`
* **Header**: `Authorization: Bearer <token>`
* **Request Body**:
    ```json
    {
      "jwe_envelope": "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9..."
    }
    ```
* **鉴权**: 服务端验证 Token 中的 `user_did` 与 `agent_id` 的绑定关系。

### 5.3 消息拉取接口 (上行)

* **Endpoint**: `GET /v1/sync/messages/{msg_id}`
* **Header**: `Authorization: Bearer <token>`
* **鉴权**: 服务端验证 Token 中的 `user_did` 与消息归属用户一致，防止越权拉取他人消息。MCP/Skill 侧通过 WebSocket 连接时使用 DIDComm 密钥认证。
* **Response**:
    ```json
    {
      "msg_id": "uuid-123",
      "jwe_envelope": "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9...",
      "created_at": 1712739265
    }
    ```

### 5.4 批量同步机制 (兜底方案)

为防止 FCM 信号丢失或 WS 离线期间遗漏消息，App 在**回到前台**时必须调用：

* **Endpoint**: `GET /v1/sync/list?after={last_read_id}&limit=100`
* **作用**: 返回 `last_read_id` 之后所有未读的消息列表。
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

> **完整同步**：若 App 丢失本地数据（重装/换设备），使用 `GET /v1/sync/list?after=&limit=100`（不传 `after` 参数）从最早消息开始同步。服务端按 `user_did` 过滤，确保只返回该用户的消息。

---

## 6. 开发实现指导

### 6.1 手机端 (Flutter)

* **安全存储**: 私钥必须存放在 `flutter_secure_storage`（Android 使用 KeyStore，iOS 使用 Keychain）。
* **加解密性能**: 必须使用 **Rust FFI** 处理 DIDComm 逻辑。在 Flutter 中使用 `Worker Isolate` 调用，避免阻塞主线程导致动画掉帧。
* **推送监听**:
    * **海外用户**：需同时处理 `FirebaseMessaging.onMessage` (前台) 和 `FirebaseMessaging.onBackgroundMessage` (后台)。
    * **中国用户**：维持与 Mediator 的 WebSocket 长连接，监听 `onWebSocketMessage` 实时接收 JWE。App 切后台时 WS 可能断开，回到前台后需自动重连并执行 Message Pickup (`status-request` / `batch-pickup`) 拉取离线期间暂存的消息。

> **iOS 注意事项**：iOS 上 FCM 依赖 APNs，当 App 被 Force Quit 后推送可能延迟或无法送达。5.4 的批量同步机制是 iOS 端的必要兜底——App 回到前台时必须触发一次同步，确保不遗漏消息。

### 6.2 服务端 (Mediator)

* **缓存策略**: 建议 Redis 存储 JWE 缓存，设置 **7 天 TTL**。
* **推送优化**: 针对不同消息类型，配置 FCM 的 `Priority`。控制指令反馈用 `High`，普通状态更新用 `Normal`。
* **WebSocket 鉴权**:
    - MCP/Skill 使用 DIDComm Agent 密钥签名进行 WebSocket 连接认证。
    - 连接建立时执行 DIDComm Mediator 协议握手（`coordinate-mediation/2.0/mediate-request` → `mediate-grant` → `keylist-update`）。
    - 服务端 (Mediator) 根据握手时注册的 DID 路由消息，确保消息仅发送到已认证的连接。

### 6.3 MCP/Skill 终端

* **WebSocket 协议**: 使用 **DIDComm V2** 标准的 WebSocket 连接，遵循 `coordinate-mediation/2.0` 协议握手（与 `ignite-pay-did.md` §4.1 一致）。
* **重连机制**: WebSocket 断连后每 3 秒自动重连并重新执行完整握手（`mediate-request` → `mediate-grant` → `keylist-update` → `peer-did-discovery`）。
* **离线消息**: 重连后通过 Message Pickup 3.0 协议 (`status-request` / `batch-pickup`) 拉取离线期间的消息。超过 7 天的离线消息通过 `GET /v1/sync/list` 补全。

---

## 7. 安全与校验规范

| 校验环节 | 实现方式 | 目的 |
| :--- | :--- | :--- |
| **消息来源校验** | DIDComm `Unpack` 结果中的 `authenticated` 属性 | 确认消息确实由绑定的 MCP/Skill 发出。 |
| **防重放校验** | 记录并检查 DIDComm Message 的 `id` (Unique Message ID) | 防止恶意拦截消息后重复提交。 |
| **时效校验** | 检查消息体内的 `expires_time` (Expiration) | 丢弃由于网络极端延迟导致的过时指令。 |
| **传输加密** | 全链路 TLS 1.3 | 保护外层元数据，配合 DIDComm 实现双层加密。 |
| **API 鉴权** | HTTPS Bearer Token (JWT) + 用户-MCP/Skill 绑定校验 | 防止越权访问他人消息。 |
| **WebSocket 鉴权** | DIDComm 密钥签名 + Mediator 协议握手 | 防止冒充 MCP/Skill 连接或窃听消息。 |

---

## 8. WebSocket 消息路由

MCP/Skill 通过 DIDComm Mediator 协议（`coordinate-mediation/2.0`）建立持久 WebSocket 连接，消息路由基于 DID 而非 Topic：

| 方向 | 消息类型 | DIDComm 协议 | 说明 |
|:-----|:---------|:-------------|:-----|
| MCP/Skill → Mediator | `mediate-request` | Coordinate Mediation 2.0 | 注册为 Mediator 客户端 |
| MCP/Skill → Mediator | `keylist-update` | Coordinate Mediation 2.0 | 注册接收密钥 `{did}#key-1` |
| MCP/Skill → Mediator | `forward` | Routing 2.0 | 转发 DIDComm 消息至目标 |
| Mediator → MCP/Skill | JWE Message | DIDComm V2 | 投递加密消息 |
| 手机 (中国) → Mediator | `mediate-request` | Coordinate Mediation 2.0 | 中国用户 WS 连接注册 |
| 手机 (中国) → Mediator | `keylist-update` | Coordinate Mediation 2.0 | 注册用户 DID 接收密钥 |
| Mediator → 手机 (中国) | JWE Message | DIDComm V2 | WS 在线直推加密消息 |
| 手机 (中国) → Mediator | `status-request` | Message Pickup 3.0 | 查询离线期间暂存消息数量 |
| Mediator → 手机 (中国) | `status` | Message Pickup 3.0 | 返回暂存消息计数 |
| 手机 (中国) → Mediator | `batch-pickup` | Message Pickup 3.0 | 批量拉取离线暂存消息 |
| Mediator → 手机 (中国) | `batch` | Message Pickup 3.0 | 返回批量消息 |

> **路由机制**：Mediator 根据消息目标的 DID 查找已注册的 WebSocket 连接，将消息投递到对应的连接。未在线的客户端消息暂存至 Redis（7 天 TTL），客户端重连后通过 Message Pickup 3.0 协议拉取。

---

## 9. 中国用户 WebSocket 通道

### 9.1 背景

中国大陆用户无法使用 Google FCM，当前"FCM 门铃 + HTTPS 拉取"模式不适用。需要为中国用户提供纯 WebSocket 通道：手机端维持与 Mediator 的 WS 长连接，消息直接通过 WS 实时推送。

### 9.2 双通道架构

```
海外用户: MCP → WS → Mediator → FCM 信号 → 手机 HTTPS 拉取 (当前)
中国用户: MCP → WS → Mediator → WS 直推 → 手机实时接收 (新增)
```

### 9.3 判断中国用户

手机在注册时通过以下信号判断（任一命中即视为中国用户）：
1. `Locale` 包含 `zh_CN` 或语言为 `zh` 且国家为 `CN`
2. 时区为 `Asia/Shanghai`、`Asia/Chongqing` 等

### 9.4 注册流程差异

- **中国用户**: 注册 `push_channel: "websocket"`，不注册 FCM token
- **海外用户**: 注册 `push_channel: "fcm"` + FCM token（不变）

### 9.5 路由策略

当需要向手机推送消息时：
1. 查询用户的 `push_channel` 偏好
2. `"websocket"`: 检查用户 WS session 是否在线
   - 在线: 直接通过 WS 推送 JWE
   - 离线: 存入 message queue（手机重连后通过 Pickup 协议拉取）
3. `"fcm"`: 走现有 FCM 信号 + HTTPS 拉取逻辑

---

## 10. 下一步行动建议

1.  **集成 Firebase**: 完成 Flutter 与 FCM 的基础对接。
2.  **编写 FFI 桥接**: 封装 Rust 的 `didcomm-rs` 到 Flutter 可用的库。
3.  **实现 WebSocket Mediator**: 部署支持 DIDComm Coordinate Mediation 2.0 的 WebSocket 服务端，配置 TLS 和 DID 密钥认证（与 `ignite-pay-did.md` §4.1 握手协议一致）。
4.  **实现 Mediator API**: 包含指令提交（HTTPS）、消息暂存（Redis）、批量同步三个核心接口。
5.  **密钥管理流程**: 实现用户注册时的 DID 生成、MCP/Skill 绑定、密钥轮换通知机制。
