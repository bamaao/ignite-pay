# Ignite Pay — 产品文档

## 一、系统概览

Ignite Pay 是一套基于 Solana 的去中心化支付系统，由三个核心组件构成：

```
┌─────────────┐    DIDComm    ┌──────────┐    DIDComm    ┌──────────────┐
│  用户端 App  │◄────────────►│ Mediator │◄────────────►│  商户端 App   │
│ (Sentinel)  │   加密消息    │  中继服务 │   加密消息    │  (Merchant)  │
└──────┬──────┘               └──────────┘               └──────┬───────┘
       │                                                        │
       │  State Channel API            State Channel API        │
       ▼                                                        ▼
┌─────────────────────────────────────────────────────────────────────┐
│                        Hub (支付通道服务)                            │
│   /v1/channels/open · pay · close · settle · claim · finalize      │
└─────────────────────────────────────────────────────────────────────┘
       │
       ▼
┌─────────────────┐
│  Solana 区块链   │
│  状态通道合约     │
│  Session Key 合约 │
└─────────────────┘
```

**核心理念：** 用户通过手机 App 掌控自己的 DID 身份和资金，对 AI 代理发起的支付进行实时授权；商户通过手机 App 生成收款码并接收即时支付确认。两端通过 DIDComm 端到端加密通信，经 Mediator 中继。

---

## 二、用户端 App — Sentinel Dashboard

### 2.1 产品定位

用户的"支付守卫"App。管理 DID 身份、接收并审批 AI 代理发起的支付请求、管理 Session Key、维护白名单/黑名单策略。

### 2.2 技术栈

| 层级 | 技术 |
|------|------|
| UI 框架 | Flutter (Dart) |
| 加密/存储 | Rust (via flutter_rust_bridge) |
| 本地数据库 | sled (身份、Session Key、策略、通道) + SQLite (审计日志) |
| 消息传输 | DIDComm v2 (JWE authcrypt 加密) |
| 推送通道 | FCM (海外) / WebSocket 长连接 (国内) |
| 区块链 | Solana (State Channel 合约 + Session Key 合约) |

### 2.3 核心身份

用户拥有一个 `did:ignite:z<multicodec_base58>` 去中心化身份，包含：
- Ed25519 签名密钥
- X25519 密钥协商密钥（用于 JWE 加解密）
- W3C DID Document

身份存储在 sled 本地数据库，首次启动时生成。

### 2.4 功能模块

#### 2.4.1 引导流程 (OnboardingScreen)

三步引导：欢迎页 → 生成 DID 身份 → 配置 Mediator 连接（可跳过）。

#### 2.4.2 仪表盘 (Dashboard)

- DID 身份卡片（显示 DID、连接状态、待处理消息数）
- 快捷入口：Vault（密钥库）、Policies（策略管理）
- "Scan MCP QR Code" 按钮：扫描商户二维码建立配对
- 信任额度仪表盘：当日消费/限额
- 最近活动列表
- 支付授权横幅：收到 `payment-auth-request` 时弹出

#### 2.4.3 支付授权 (ChallengeScreen — X402 协议)

核心交互界面。收到 MCP 支付请求后弹出全屏授权页：

1. **展示信息**：商户 DID、金额 (SOL)、支付原因
2. **策略配置**：每日限额、每日笔数上限、单笔限额、有效期
3. **名单操作**：仅本次 / 加入白名单 / 加入黑名单 / 移除白名单 / 移除黑名单
4. **签名方式选择**：内置密钥 / Phantom 深链接 / Solflare 深链接 / MWA (Android)
5. **操作**：Approve（创建 Session Key → 链上注册 → 回复 MCP）或 Decline & Block

#### 2.4.4 策略管理 (PolicyScreen)

按商户配置消费规则：
- 自动支付开关
- 单笔限额 (SOL/USD 切换)
- 周消费额度进度条
- 有效期倒计时

#### 2.4.5 密钥库 (VaultScreen)

- DID 身份展示（带动画渐变背景）
- 12 词助记词（可显示/隐藏）
- Mediator 端点配置
- 审计日志查看
- "Erase Key Material" 危险操作

#### 2.4.6 Session Key 管理 (SessionKeysScreen)

链上 Session Key 全生命周期：
- 注册新 Key（5 SOL / 24h 默认参数）
- 查看活跃/已过期 Key 列表
- 撤销（链上）/ 删除（本地）

#### 2.4.7 QR 扫描 (QrScannerScreen)

全屏相机扫描 `didcomm://?_oob=<base64>` 格式的 MCP 配对二维码。扫描后解析 OOB 邀请，连接 Mediator，发送 `connection-request`。

#### 2.4.8 通道支付 (QrPaymentScreen)

扫描商户收款码 `ignite://pay?d=<base64>` 后的确认页：
- 展示商户信息、金额、描述
- Confirm → 调用 Hub API 执行状态通道支付
- 显示支付结果（sequence, leaf_index）

#### 2.4.9 消息列表 (MessagesScreen)

DIDComm 消息收件箱：
- 筛选标签：全部 / Payment / List Sync / Connection
- 点击 `payment-auth-request` 消息触发授权流程
- 查看消息详情（含 raw body）

#### 2.4.10 连接管理 (ConnectionScreen)

- Mediator 连接状态（WS/FCM 通道显示）
- 已配对的 MCP 代理列表
- 添加新 MCP（打开 QR 扫描器）

#### 2.4.11 设置 (SettingsScreen)

- Solana 网络：Devnet / Mainnet 切换、RPC URL、DAS Endpoint
- SPL 账户压缩参数：Tree Address、Tree Authority
- 程序 ID：State Channel、DID、Session Key（只读）
- 支付模式：自费 / 赞助
- 存储管理：清除缓存

### 2.5 核心服务

| 服务 | 职责 |
|------|------|
| `DidcommService` | DID 身份管理、Mediator 连接、消息收发、认证、推送编排 |
| `SessionKeyService` | Session Key 创建/注册/撤销/查询，支持内置密钥和外接钱包 |
| `ChannelService` | 状态通道操作（解析 QR、支付、列表） |
| `FcmService` | Firebase 推送通知 |
| `MediatorApi` | Mediator REST API HTTP 客户端 |
| `WalletDeepLinkService` | Phantom / Solflare 钱包深度链接构建 |
| `WalletMwaService` | Mobile Wallet Adapter（Android，桩实现） |

### 2.6 Rust Bridge 函数清单

| 函数 | 用途 |
|------|------|
| `initialize_identity` | 生成/加载 DID 身份 |
| `connect_mediator` | 连接 Mediator WebSocket |
| `disconnect_mediator` | 断开连接 |
| `authenticate_with_mediator` | 挑战-响应认证获取 JWT |
| `pull_messages` | 从 Mediator 拉取 JWE 消息 |
| `decrypt_message` | 解密 JWE 提取支付字段 |
| `send_auth_response` | 发送支付授权响应（含 Session Key） |
| `register_device_token` | 注册 FCM 设备令牌 |
| `parse_oob_invitation` | 解析 OOB 邀请二维码 |
| `send_connection_request` | 发送连接请求到 MCP |
| `create_session_key_for_payment` | 创建临时 Session Key |
| `build_unsigned_register_tx` | 构建未签名注册交易（外接钱包） |
| `complete_register_with_signature` | 完成注册 |
| `revoke_session_key_onchain` | 链上撤销 Session Key |
| `save_merchant_policy` / `load_merchant_policy` | 商户策略持久化 |
| `parse_payment_qr` | 解析收款二维码 |
| `open_channel` / `channel_pay` / `close_channel` / `settle_channel` | 状态通道操作 |

### 2.7 支付授权完整流程

```
1. MCP 代理发起 payment-auth-request
2. Mediator 推送到用户 App（FCM 或 WS）
3. App 拉取 JWE → 解密 → 展示授权请求
4. 用户在 ChallengeScreen 审核：
   a. 确认金额、商户
   b. 可选：调整策略参数
   c. 可选：添加白名单/黑名单
   d. 选择签名方式
5. App 创建 Session Key → 链上注册交易 → 确认
6. 发送 payment-auth-response（含 Session Key 数据）回 MCP
7. MCP 使用 Session Key 执行支付
```

---

## 三、商户端 App — Ignite Merchant

### 3.1 产品定位

商户的收款工具。生成收款二维码、接收即时支付确认、管理状态通道、语音播报到账。

### 3.2 技术栈

| 层级 | 技术 |
|------|------|
| UI 框架 | Flutter (Dart) |
| 加密/存储 | Rust (via flutter_rust_bridge) |
| 本地数据库 | sled (订单、密钥、通道、DIDComm 身份) |
| 消息传输 | DIDComm v2 (JWE authcrypt 加密) |
| 推送通道 | FCM (海外) / WebSocket 长连接 (国内) |
| 语音播报 | flutter_tts (中英双语) |

### 3.3 双 DID 架构

商户 App 管理两个独立身份：

| 身份 | DID 格式 | 用途 | 存储位置 |
|------|----------|------|----------|
| 状态通道 DID | `did:ignite:<raw_base58>` | QR 码生成、通道操作、链上签名 | sled `keypairs` tree |
| DIDComm 通信 DID | `did:ignite:z<multicodec_base58>` | JWE 加解密、Mediator 消息收发 | sled `didcomm_identity` tree |

两者密钥体系完全独立，互不干扰。

### 3.4 功能模块

#### 3.4.1 引导流程 (OnboardingScreen)

1. 填写 Hub Endpoint URL
2. 填写 Mediator WebSocket URL（可选）
3. 生成商户身份（Ed25519 密钥对 → 状态通道 DID）
4. 初始化推送服务（连接 Mediator）

#### 3.4.2 仪表盘 (DashboardScreen)

- "Ignite Merchant" 头部 + 在线状态
- 今日汇总卡片：已收款总额 (USDC) + 笔数（仅计 confirmed）
- 快捷操作：生成收款码 / 通道管理
- 最近 5 笔订单列表

#### 3.4.3 生成收款码 (QrGenerateScreen)

核心收款界面：

1. 输入金额 (USDC) + 可选描述
2. 生成 QR 码：格式 `ignite://pay?d=<base64url(JSON)>`
3. QR 码展示，进入等待确认状态
4. **双通道等待**：
   - 推送确认（主通道）：监听 `MerchantPushService.confirmations` 流
   - 轮询兜底（5 秒间隔）：调用 `refreshOrders()` 检查订单状态
5. 确认后：绿色对勾 + 触觉反馈 + 语音播报

**QR 码载荷结构 (PaymentQrData)**：
```json
{
  "type": "ignite-pay-request",
  "version": 1,
  "merchant_did": "did:ignite:...",
  "amount": 1000000000,
  "description": "咖啡",
  "order_id": "uuid-v4",
  "hub_endpoint": "https://hub.example.com",
  "timestamp": 1713700000
}
```

#### 3.4.4 收款明细 (PaymentListScreen)

- 筛选：全部 / 待确认 / 已确认
- 下拉刷新
- 订单卡片列表（金额、状态徽章、描述、时间）

#### 3.4.5 订单详情 (PaymentDetailScreen)

- 金额展示（大字体 USDC）
- 状态徽章：confirmed=绿 / pending=琥珀 / failed=红 / expired=灰
- 订单信息：订单号（可复制）、描述、Hub、创建时间、确认时间
- 通道信息（仅 confirmed）：Channel ID（可复制）、Leaf Index、Sequence

#### 3.4.6 通道管理 (ChannelScreen)

- 汇总：通道总数 + 总余额 (USDC)
- 通道卡片列表（Channel ID、状态、Sequence、余额）
- 下拉刷新

#### 3.4.7 通道详情 (ChannelDetailScreen)

- 通道信息展示：ID、状态、Sequence、Leaf 数、余额、总存入
- 操作按钮：
  - **关闭通道**：确认对话框 → Hub API `/v1/channels/{id}/close`
  - **结算**：Claim Leaf → Finalize

#### 3.4.8 设置 (SettingsScreen)

- 商户身份：状态通道 DID（可复制）、Provider Pubkey（可复制）
- 连接配置：Hub Endpoint（可编辑）、Mediator WS（状态指示灯）
- 推送服务：DIDComm DID（可复制）、Mediator 连接状态、推送通道类型
- 语音播报：开关、语言切换（中/英）、音量滑块、测试按钮
- 关于：版本 1.0.0

### 3.5 核心服务

| 服务 | 职责 |
|------|------|
| `MerchantService` | 商户身份、订单管理、QR 生成、配置持久化 |
| `MerchantPushService` | 双通道推送编排（WS/FCM）、消息解密、订单确认 |
| `ChannelService` | 状态通道列表、关闭、结算 |
| `VoiceService` | 支付到账语音播报（中英双语） |
| `FcmService` | Firebase 推送通知 |
| `MediatorApi` | Mediator REST API HTTP 客户端 |

### 3.6 Rust Bridge 函数清单

**merchant.rs — 状态通道与订单：**

| 函数 | 用途 |
|------|------|
| `initialize_merchant` | 生成/加载商户密钥对和 DID |
| `generate_merchant_keypair` | 生成 Ed25519 密钥对 |
| `get_merchant_pubkey` | 获取 base58 公钥 |
| `generate_payment_qr` | 创建订单 + 生成 QR 字符串 |
| `list_orders` / `get_order` / `get_pending_orders` | 订单查询 |
| `confirm_order` | 订单状态 pending → confirmed |
| `merchant_list_channels` | 列出通道 ID |
| `merchant_get_channel_status` | 查询通道状态和余额 |
| `merchant_close_channel` | 关闭通道（Hub API） |
| `merchant_claim_leaf` / `merchant_finalize` | 结算流程 |

**merchant_didcomm.rs — DIDComm 通信：**

| 函数 | 用途 |
|------|------|
| `initialize_merchant_comm` | 生成/加载 DIDComm 身份（独立于状态通道 DID） |
| `connect_mediator` | 连接 Mediator |
| `disconnect_mediator` | 断开连接 |
| `authenticate_with_mediator` | 挑战-响应认证获取 JWT |
| `pull_messages` | 从 Mediator 拉取 JWE 消息 |
| `decrypt_message` | 解密 JWE 提取支付确认字段 |
| `register_device_token` | 注册 FCM 设备令牌 |

### 3.7 收款完整流程

```
1. 商户在 QR Generate 页面输入金额 + 描述
2. App 调用 Rust generate_payment_qr()
   → 创建 UUID 订单 (status=pending)
   → 返回 ignite://pay?d=... 二维码字符串
3. 用户 App 扫码 → 解析 PaymentQrData
4. 用户确认支付 → 用户 App 调用 Hub API 执行状态通道支付
5. Hub 处理支付 → 通过 DIDComm 发送 channel-payment-confirm 到 Mediator
6. Mediator 推送到商户 App（WS 或 FCM）
7. 商户 App 拉取 JWE → 解密 → 提取 order_id, channel_id, leaf_index, sequence
8. Rust confirm_order() 更新订单状态为 confirmed
9. 触发：
   - QR 页面绿色对勾
   - 触觉反馈
   - 语音播报（"收到收款 X.XX USDC"）
   - Dashboard 今日汇总刷新
```

---

## 四、共享基础设施

### 4.1 ignite-pay-core

核心协议库，提供：

| 模块 | 功能 |
|------|------|
| `identity` | DID 生成、DID Document 构建、身份持久化、DID 签名验证 |
| `didcomm` | DIDComm 消息构造器（15 种消息类型）、JWE 加解密、Agent 创建 |
| `types` | 共享类型：PaymentRequest, MerchantListEntry, VerifiableCredential, RiskControlDecision |
| `list_store` | 白名单/黑名单管理（sled + IPFS 同步），风控决策 |
| `vc` | Verifiable Credential 签发与验证 |
| `ipfs` | IPFS 上传/下载抽象层 |
| `audit_merkle` | SHA-256 Merkle 树审计日志 |
| `log_crypto` / `log_chunk` / `log_sync` | E2EE 审计日志（加密 → Zstd 压缩 → IPFS 同步） |

### 4.2 ignite-pay-state-channel

状态通道协议库，提供：

| 模块 | 功能 |
|------|------|
| `types` | UTXOLeaf (Standard/HTLC/Compliance), LeafUpdate, SignedState, ChannelMetadata |
| `merkle` | 二叉 Merkle 树（sorted-pair hashing，匹配链上 compression.rs） |
| `channel` | ChannelManager：开通道、应用更新、批量更新、联合签名、争议/结算 |
| `pipeline` | 原子批量操作构建器：转账、部分转账、创建 HTLC、解锁、退款 |
| `htlc` | HTLC 生命周期管理（创建/揭示/过期/退款） |
| `signing` | Ed25519 签名和验证（叶更新、状态根、Claim） |
| `hub` | Hub 注册和指标管理 |
| `routing` | 多跳路由发现和评分 |
| `multihop` | 多跳支付执行（递减 timelock） |
| `compliance` | 合规标记（滑动窗口消费阈值、Travel Rule 数据） |

### 4.3 DIDComm 消息类型

| 消息 | 方向 | 用途 |
|------|------|------|
| `out-of-band/2.0/invitation` | MCP → 用户 | QR 配对邀请 |
| `ignite-pay/1.0/connection-request` | 用户 → MCP | 建立连接 |
| `ignite-pay/1.0/connection-response` | MCP → 用户 | 连接确认 |
| `ignite-pay/1.0/payment-auth-request` | MCP → 用户 | 请求支付授权 |
| `ignite-pay/1.0/payment-auth-response` | 用户 → MCP | 授权响应（含 Session Key） |
| `ignite-pay/1.0/channel-payment-request` | — | 状态通道支付请求 |
| `ignite-pay/1.0/channel-payment-confirm` | — | 状态通道支付确认 → 推送给商户 |
| `ignite-pay/1.0/list-sync-notification` | MCP → 用户 | 白名单/黑名单更新 |
| `coordinate-mediation/2.0/*` | 双向 | Mediator 协议（mediate-request, keylist-update） |
| `ignite-pay/1.0/ws-challenge-response` | 双向 | WS 认证挑战 |
| `messagepickup/3.0/*` | 双向 | 消息拾取协议 |

### 4.4 Mediator REST API

| 端点 | 方法 | 用途 |
|------|------|------|
| `/v1/auth/challenge` | GET | 获取认证 nonce |
| `/v1/auth/token` | POST | 签名换 JWT |
| `/v1/sync/list` | GET | 拉取消息列表（游标分页） |
| `/v1/sync/messages/{id}` | GET | 获取单条消息 |
| `/v1/agents/{id}/command` | POST | 发送加密命令 |
| `/v1/agents/bind` | POST | 绑定 Agent DID |
| `/v1/devices/register-token` | POST | 注册推送通道（FCM token 或 websocket） |

### 4.5 Hub REST API

| 端点 | 方法 | 用途 |
|------|------|------|
| `/v1/channels/open` | POST | 开通状态通道 |
| `/v1/channels/{id}/pay` | POST | 通道支付 |
| `/v1/channels/{id}/close` | POST | 协作关闭 |
| `/v1/channels/{id}/settle` | POST | 发起结算 |
| `/v1/channels/{id}/claim` | POST | 认领叶子 |
| `/v1/channels/{id}/finalize` | POST | 完成结算 |

---

## 五、双端对比

| 维度 | 用户端 (Sentinel) | 商户端 (Merchant) |
|------|-------------------|-------------------|
| **核心角色** | 支付授权守卫 | 收款工具 |
| **DID 数量** | 1 个（通信 + 交易共用） | 2 个（状态通道 DID + DIDComm DID） |
| **消息方向** | 收 auth-request → 发 auth-response | 收 payment-confirm → 确认订单 |
| **QR 交互** | 扫码（配对 MCP / 支付） | 生成码（收款） |
| **链上操作** | Session Key 注册/撤销 | 无直接链上操作 |
| **推送触发** | MCP 支付请求 | 支付确认通知 |
| **特色功能** | 白名单/黑名单策略、外接钱包签名 | 语音播报、订单管理 |
| **UI 语言** | 英文 | 中文 |
| **屏幕数** | 11 | 8 |
| **Rust 模块** | simple + identity + auth + session + ws_client + channel + channel_store + log_store (8) | merchant + merchant_didcomm (2) |
| **Bridge 函数** | 30+ | 24 |

---

## 六、设计系统

两个 App 共享同一套 Dark Glassmorphism 设计语言：

| Token | 值 | 用途 |
|-------|-----|------|
| Background | `#0A0A14` | 页面背景 |
| Surface | `#12121F` ~ `#22223A` | 卡片、输入框、边框 |
| Text Primary | `#F0F0F8` | 标题、金额 |
| Text Secondary | `#7A7A96` | 描述 |
| Neon Cyan | `#00F5FF` | 主强调色、按钮渐变 |
| Purple | `#8B5CF6` | 次强调色 |
| Success | `#00E676` | 已确认、已连接 |
| Pending | `#FFB300` | 待处理 |
| Danger | `#FF5252` | 失败、关闭、断开 |

字体：Inter（正文）+ JetBrains Mono（数值、DID、代码）。

共享组件：`BackButtonGlass`, `PageHeader`, `SettingsTile`, `SectionLabel`, `glassDecoration()`。

---

## 七、数据模型

### 7.1 订单 (PaymentOrder)

```
状态流转：pending → confirmed / failed / expired

字段：
  orderId        String      UUID v4
  merchantDid    String      did:ignite:...
  amount         BigInt      lamports (1 USDC = 1_000_000_000)
  description    String      可选描述
  hubEndpoint    String      Hub API URL
  status         String      "pending" | "confirmed" | "failed" | "expired"
  createdAt      int         Unix 秒
  confirmedAt    int?        Unix 秒（仅 confirmed）
  channelId      String?     通道 ID（仅 confirmed）
  leafIndex      int?        Merkle 树叶索引（仅 confirmed）
  sequence       BigInt?     通道序列号（仅 confirmed）
```

### 7.2 通道 (ChannelInfo)

```
字段：
  channelId        String
  status           String      "Open" | "Closed" | "Settling" | "Unknown"
  sequence         BigInt
  leafCount        int
  providerBalance  BigInt      lamports（商户余额）
  totalDeposited   BigInt      lamports
```

### 7.3 DIDComm 消息 (DecryptedMessage)

```
商户端解密后字段：
  msgType      String      消息类型 URI
  orderId      String?     关联订单 ID
  channelId    String?     通道 ID
  leafIndex    int?        叶索引
  sequence     BigInt?     序列号
  amount       BigInt?     确认金额
  authorized   bool?       授权状态
  rawBody      String      原始 JSON
```

---

## 八、推送通知架构

```
                    ┌───────────────────────┐
                    │      Mediator         │
                    │   (消息中继 + 推送)    │
                    └─────┬──────────┬──────┘
                          │          │
              ┌───────────┘          └───────────┐
              │                                  │
        zh_CN 用户                          非 zh_CN 用户
              │                                  │
     WebSocket 长连接                      FCM 推送通知
     (直接接收 JWE)                     (SIGNAL → 拉取 JWE)
              │                                  │
              └──────────┬───────────────────────┘
                         │
                    pull_messages()
                    decrypt_message()
                    确认订单 / 授权支付
```

**WS 流程**（国内用户）：连接 → identify → 持续监听 → 收到 JWE → 直接解密处理

**FCM 流程**（海外用户）：SIGNAL 通知 → `onSignalReceived` → pull_messages 拉取 → 解密处理

**共同点**：
- 首次连接时 authenticate → pull 离线消息
- WS 断线后先拉离线消息再重连（3 秒延迟）
- FCM 前台收到时显示本地通知（标题 "Payment Received"）

---

## 九、安全模型

| 安全措施 | 说明 |
|----------|------|
| DID 身份 | Ed25519 签名密钥 + X25519 密钥协商，本地 sled 加密存储 |
| 消息加密 | DIDComm authcrypt (JWE)，端到端加密，Mediator 无法读取明文 |
| Mediator 认证 | 挑战-响应：nonce → Ed25519 签名 → JWT 令牌 |
| Session Key | 临时密钥链上注册，有效期和额度限制，可撤销 |
| 白名单/黑名单 | IPFS 同步的商户名单，风控决策 (Blocked/AutoApproved/NeedsAuth) |
| 审计日志 | Merkle 树 + E2EE 加密 + IPFS 同步，防篡改 |
| 双 DID 隔离 | 状态通道 DID 和通信 DID 分离，互不影响 |
