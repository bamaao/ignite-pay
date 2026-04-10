**基于 DIDComm V2 的 AI Agent 全链路通信方案**。

它整合了 **FCM 信号唤醒**、**HTTPS 主动拉取**以及 **MQTT 终端控制**，是一个专为高安全、跨国环境（Google Play & iOS）设计的生产级架构。

---

# AI Agent 全链路通信系统技术规约 (DIDComm V2 标准)

## 1. 架构逻辑：门铃与包裹模式 (Signal & Pull)

为了规避 FCM 载荷限制并确保 **DIDComm V2** 的端到端加密安全，系统采用"门铃与包裹"逻辑：

* **FCM (门铃)**：仅发送一个轻量级的通知信号，告诉手机"你有新消息"。
* **HTTPS (包裹)**：手机端通过安全的 HTTPS 通道，主动从服务端拉取完整的加密 DIDComm Message (JWE) 包。
* **DIDComm (拆包)**：加解密与验签完全在手机和 Agent 本地完成，服务端无法解密消息体。

> **传输路径不对称说明**：
> - **下行链路**（手机→Agent）：手机使用 HTTPS 提交指令（省电，无需维持长连接），服务端通过 MQTT 实时推送给 Agent（Agent 常在线）。
> - **上行链路**（Agent→手机）：Agent 通过 MQTT 上报结果（实时性好），服务端通过 FCM 信号通知手机，手机再通过 HTTPS 拉取（避免手机维持长连接消耗电量）。

---

## 2. 系统拓扑图

```
┌─────────────────────────────────────────────────────────────────────────┐
│                                                                         │
│    手机 (Flutter App)                 服务端 (Mediator)        Agent 终端 │
│                                                                         │
│    ┌──────────────┐              ┌──────────────┐           ┌────────┐ │
│    │ DIDComm      │   HTTPS      │ Mediator     │   MQTT    │ Agent  │ │
│    │ Pack/Unpack  │──────────────│              │───────────│        │ │
│    │              │<─────────────│ - 消息暂存    │<──────────│ DIDComm│ │
│    │ FCM Listener │              │ - FCM 推送    │           │        │ │
│    └──────────────┘              │ - MQTT Broker │           └────────┘ │
│         ↑                        └──────────────┘                      │
│         │ FCM Signal                                                   │
│    ┌──────────────┐                                                    │
│    │ FCM / APNs   │<─────────────── FCM 推送 ──────────────────────     │
│    └──────────────┘                                                    │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘

消息流向:

  下行 (手机→Agent):                    上行 (Agent→手机):
    Flutter → HTTPS POST → Mediator      Agent → MQTT Publish → Mediator
    → MQTT Publish → Agent               → Redis 暂存 → FCM Signal
    → DIDComm Unpack → 执行               → HTTPS GET → Flutter
                                         → DIDComm Unpack → UI 更新
```

---

## 3. 身份与密钥管理

### 3.1 身份模型

系统使用 **DID** 作为身份标识，每个参与者拥有唯一的 DID 和关联的密钥对：

| 角色 | 身份标识 | 密钥用途 |
|:-----|:---------|:---------|
| 手机 (User) | `did:ignite:user_{uuid}` | 签名指令、解密来自 Agent 的消息 |
| Agent | `did:ignite:agent_{uuid}` | 签名反馈、解密来自手机的指令 |
| 服务端 (Mediator) | `did:ignite:mediator` | 不参与 DIDComm 加解密，仅路由转发 |

### 3.2 密钥交换与信任建立

首次使用前，需完成以下绑定流程：

```
手机 (Flutter)                        服务端 (Mediator)                Agent
    │                                       │                              │
    │  1. 用户注册/登录                       │                              │
    │  (生成 DID + Ed25519 密钥对,            │                              │
    │   私钥存入 flutter_secure_storage)       │                              │
    │ ──────────────────────────────────────>│                              │
    │  { did: "did:ignite:user_xxx",          │                              │
    │    public_key: <Ed25519 pubkey> }        │                              │
    │                                       │                              │
    │  2. 绑定 Agent                         │                              │
    │ ──────────────────────────────────────>│                              │
    │  { agent_id: "did:ignite:agent_yyy" }  │                              │
    │                                       │                              │
    │                                       │  3. 服务端验证绑定关系         │
    │                                       │     (用户是否有权控制此 Agent)  │
    │                                       │                              │
    │  4. 返回 Agent 的 DID 文档              │                              │
    │ <──────────────────────────────────────│                              │
    │  { did: "did:ignite:agent_yyy",         │                              │
    │    public_key: <Agent Ed25519 pubkey>,   │                              │
    │    mqtt_endpoint: "mqtts://..." }        │                              │
    │                                       │                              │
    │  5. 手机本地缓存 Agent 公钥              │                              │
    │     (后续 DIDComm 加密使用此公钥)        │                              │
```

> **密钥轮换**：当任一方轮换密钥时，通过 DID 文档更新通知对端。服务端维护最新的 DID 文档缓存。

---

## 4. 全链路交互详述

### 4.1 下行链路：手机控制 Agent

1.  **消息封装 (Flutter)**：使用本地存储的私钥对指令签名，并针对 Agent 的公钥进行加密，生成 DIDComm Encrypted Message (JWE)。
2.  **指令提交 (HTTPS)**：通过 `POST /v1/agents/{agent_id}/command` 将 JWE 提交至服务端。`agent_id` 在 URL 路径中指定。
3.  **鉴权与路由 (Server)**：服务端验证 Bearer Token 对应的用户是否有权向该 `agent_id` 发送指令。验证通过后，将 JWE 发布到 MQTT Topic：`agents/{agent_id}/inbound`。
4.  **执行 (Agent)**：Agent 终端接收消息，本地解密验签后执行。

### 4.2 上行链路：Agent 反馈至手机 (核心可靠方案)

1.  **结果封装 (Agent)**：执行完成后，Agent 生成加密反馈 JWE（使用用户公钥加密）。
2.  **数据上报 (MQTT)**：Agent 发布 JWE 到 Topic：`agents/{agent_id}/outbound`。
3.  **暂存与信号 (Server)**：
    * 服务端接收并存入缓存（Redis/DB），生成 `msg_id`。
    * 服务端根据 `agent_id` 查找绑定的用户，验证消息归属。
    * 调用 **FCM** 发送 Data Message：`{"type": "SIGNAL", "msg_id": "uuid-123"}`。
4.  **主动拉取 (Flutter)**：
    * Flutter 收到 FCM 信号，发起请求：`GET /v1/sync/messages/uuid-123`。
5.  **解密展示 (Flutter)**：获取 JWE 后，在本地 Isolate 中进行 DIDComm `Unpack`，校验成功后更新 UI。

---

## 5. 关键接口与协议定义

### 5.1 双层认证说明

系统使用两层独立的认证机制，各司其职：

| 层级 | 机制 | 目的 | 验证方 |
|:-----|:-----|:-----|:-------|
| **传输层** | HTTPS Bearer Token (JWT) | 验证"谁在调用 API"——确保只有合法用户能拉取/提交消息 | 服务端 (Mediator) |
| **消息层** | DIDComm 签名 (`Unpack` 的 `authenticated`) | 验证"消息是谁发的"——确认消息确实由绑定的 Agent/用户发出 | 客户端 (Flutter/Agent) |

Bearer Token 是用户登录后服务端签发的 JWT，包含 `user_did` 字段。Token 与 DID 身份绑定：服务端通过 Token 中的 `user_did` 查找其绑定的 Agent 列表，实现鉴权。

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
* **鉴权**: 服务端验证 Token 中的 `user_did` 与消息归属用户一致，防止越权拉取他人消息。
* **Response**:
    ```json
    {
      "msg_id": "uuid-123",
      "jwe_envelope": "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9...",
      "created_at": 1712739265
    }
    ```

### 5.4 批量同步机制 (兜底方案)

为防止 FCM 信号丢失，App 在**回到前台**时必须调用：

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
* **推送监听**: 需同时处理 `FirebaseMessaging.onMessage` (前台) 和 `FirebaseMessaging.onBackgroundMessage` (后台)。

> **iOS 注意事项**：iOS 上 FCM 依赖 APNs，当 App 被 Force Quit 后推送可能延迟或无法送达。5.4 的批量同步机制是 iOS 端的必要兜底——App 回到前台时必须触发一次同步，确保不遗漏消息。

### 6.2 服务端 (Mediator)

* **缓存策略**: 建议 Redis 存储 JWE 缓存，设置 **7 天 TTL**。
* **推送优化**: 针对不同消息类型，配置 FCM 的 `Priority`。控制指令反馈用 `High`，普通状态更新用 `Normal`。
* **MQTT 鉴权 (ACL)**:
    - Agent 使用 TLS 客户端证书 + `agent_id` 认证连接 MQTT Broker。
    - ACL 规则：Agent 只能发布到 `agents/{自身 agent_id}/outbound`，订阅 `agents/{自身 agent_id}/inbound`。
    - 服务端 (Mediator) 使用特权账号，可发布到所有 `agents/*/inbound`，订阅所有 `agents/*/outbound`。

### 6.3 Agent 终端

* **MQTT 协议**: 使用 **MQTT 5.0**，设置 `Clean Start = false`，`Session Expiry Interval = 86400`（24 小时）。
* **重连机制**: MQTT 必须实现自动重连。
* **离线消息**: MQTT Broker 配置 `message_expiry_interval = 604800`（7 天），与 Redis TTL 一致。超过 7 天的离线消息自动丢弃，Agent 上线后通过 `GET /v1/sync/list` 补全。

---

## 7. 安全与校验规范

| 校验环节 | 实现方式 | 目的 |
| :--- | :--- | :--- |
| **消息来源校验** | DIDComm `Unpack` 结果中的 `authenticated` 属性 | 确认消息确实由绑定的 Agent 发出。 |
| **防重放校验** | 记录并检查 DIDComm Message 的 `id` (Unique Message ID) | 防止恶意拦截消息后重复提交。 |
| **时效校验** | 检查消息体内的 `expires_time` (Expiration) | 丢弃由于网络极端延迟导致的过时指令。 |
| **传输加密** | 全链路 TLS 1.3 | 保护外层元数据，配合 DIDComm 实现双层加密。 |
| **API 鉴权** | HTTPS Bearer Token (JWT) + 用户-Agent 绑定校验 | 防止越权访问他人消息。 |
| **MQTT ACL** | TLS 客户端证书 + Topic 级权限控制 | 防止冒充 Agent 发布或窃听消息。 |

---

## 8. MQTT Topic 规范

统一使用以下层级结构，确保多用户隔离：

| Topic | 发布者 | 订阅者 | 用途 |
|:------|:-------|:-------|:-----|
| `agents/{agent_id}/inbound` | Mediator | Agent | 下行指令（手机→Agent） |
| `agents/{agent_id}/outbound` | Agent | Mediator | 上行反馈（Agent→手机） |

> **Topic 权限**：Agent 通过 TLS 客户端证书认证，MQTT Broker 严格限制每个 Agent 只能访问包含自身 `agent_id` 的 Topic。Mediator 作为可信中间件拥有全局路由权限。

---

## 9. 下一步行动建议

1.  **集成 Firebase**: 完成 Flutter 与 FCM 的基础对接。
2.  **编写 FFI 桥接**: 封装 Rust 的 `didcomm-rs` 到 Flutter 可用的库。
3.  **实现 MQTT Broker**: 部署支持 MQTT 5.0 + ACL 的 Broker（如 EMQX 或 HiveMQ），配置 TLS 客户端证书认证。
4.  **实现 Mediator API**: 包含指令提交、消息暂存、批量同步三个核心接口。
5.  **密钥管理流程**: 实现用户注册时的 DID 生成、Agent 绑定、密钥轮换通知机制。
